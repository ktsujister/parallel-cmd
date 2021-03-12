use std::fs::File;
use std::io::{self, Read, BufReader, BufRead, Write, BufWriter};
use std::sync;
use std::sync::mpsc::{self, Sender};
use std::result::Result as StdResult;
use std::{num, thread};

use std::process::{Command, Stdio};
use std::os::unix::io::AsRawFd;

use rayon::prelude::*;
use itertools::Itertools;
use log::debug;
use libc;
use thiserror::Error;

use crate::args::AppOptions;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum SendChannel {
    In,
    Out,
}

#[derive(Debug, Error)]
pub enum CommandError {
    #[error("IO Error. Err: {0}")]
    IoError(#[from] io::Error),

    #[error("Input read error occured on line: {0}. Err: {1}")]
    InputReadError(usize, #[source] io::Error),

    #[error("Send error for {0:?} channel occured")]
    SendError(SendChannel),

    #[error("Parse int failed. Err: {0}")]
    ParseIntError(#[from] num::ParseIntError),

    #[error("Invalid command: {0:?}")]
    InvalidCommand(Vec<String>),

    #[error("Command failed. return code: {0}, err_msg: {1}")]
    CommandError(i32, String),

    #[error("Process join failed.")]
    ProcessJoinError,

    #[error("Pipe buffer size error. pipe_size: {0}, bytes_size: {1}.")]
    PipeBufferSizeError(usize, usize),

    #[error("Lock failed.")]
    LockError,
}

pub type Result<T> = StdResult<T, CommandError>;

impl<T> From<sync::PoisonError<T>> for CommandError {
    fn from(_err: sync::PoisonError<T>) -> CommandError {
        CommandError::LockError
    }
}

fn get_input_reader(input_file: &Option<String>, buffer_size: Option<usize>) -> Result<BufReader<Box<dyn Read>>> {
    let read: Box<dyn Read> = match input_file {
        Some(path) => Box::new(File::open(path.to_string())?),
        None       => Box::new(io::stdin()),
    };
    Ok(
        if let Some(buffer_size) = buffer_size {
            BufReader::with_capacity(buffer_size, read)
        } else {
            BufReader::new(read)
        }
    )
}

fn get_output_writer(output_file: &Option<String>, buffer_size: Option<usize>) -> Result<BufWriter<Box<dyn Write>>>  {
    let write: Box<dyn Write> = match output_file {
        Some(path) => Box::new(File::create(path.to_string())?),
        None       => Box::new(io::stdout()),
    };
    Ok(
        if let Some(buffer_size) = buffer_size {
            BufWriter::with_capacity(buffer_size, write)
        } else {
            BufWriter::new(write)
        }
    )
}

// /proc/sys/fs/pipe-max-size
const DESIRED_PIPE_SIZE: i32 = 1024 * 1024;

fn exec_command(options: &AppOptions, joined_part: &String) -> Result<Vec<String>> {
    let commands  = options.commands.clone();
    if commands.len() > 0 {
        let cmd = &commands[0];
        let mut command = Command::new(cmd);
        if commands.len() > 1 {
            let cmd_args = &commands[1..];
            command.args(cmd_args);
        }
        let mut cmd_proc = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        {
            let cmd_stdin = cmd_proc.stdin.as_mut().expect("stdin not available!");
            let stdin_fd = cmd_stdin.as_raw_fd();
            let pipe_size = unsafe {libc::fcntl(stdin_fd, libc::F_GETPIPE_SZ)};
            let ret_pipe_size = unsafe {libc::fcntl(stdin_fd, libc::F_SETPIPE_SZ, DESIRED_PIPE_SIZE)};
            let parts_bytes = joined_part.as_bytes();
            if options.verbose_level > 1 {
                debug!("pipe_size: {}, ret_pipe_size: {}", pipe_size, ret_pipe_size);
            }
            if ret_pipe_size < parts_bytes.len() as i32 {
                return Err(CommandError::PipeBufferSizeError(ret_pipe_size as usize, parts_bytes.len()));
            }
            // debug!("[write] before write");
            let _r = cmd_stdin.write_all(parts_bytes)?;
        }
        let output = cmd_proc.wait_with_output()?;
        if output.status.success() {
            // debug!("command succeeded! output: {:?}", &output);
            let output_str = String::from_utf8_lossy(&output.stdout);
            let str_len = output_str.len();
            let lines: Vec<String>;
            if str_len > 1 {
                let split = (&output_str[..(str_len - 1)]).split("\n");
                lines = split.map(|s| s.to_string()).collect();
            } else {
                lines = vec![];
            }
            // debug!("lines.len(): {}", lines.len());
            return Ok(lines);
        } else {
            let ecode = output.status.code().expect("no error code");
            let error_str = String::from_utf8_lossy(&output.stderr);
            return Err(CommandError::CommandError(ecode, error_str.to_string()));
        }
    } else {
        return Err(CommandError::InvalidCommand(commands));
    }
}

fn run_command_in_parallel(options: &AppOptions, partitions: Vec<String>, tx: &Sender<Vec<String>>) -> Result<usize> {
    let mut handled_count = 0;
    let tasks: Result<Vec<Vec<String>>> = partitions
        .par_iter()
        .map(|p| {
            exec_command(options, p)
        })
        .collect();
    let task_results = tasks?;
    for result in task_results {
        let out_count = result.len();
        let _r = tx.send(result).map_err(|e| {
            if options.verbose_level > 0 {
                debug!("send failed! {:?}", e);
            }
            CommandError::SendError(SendChannel::Out)
        })?;
        handled_count += out_count;
    }
    Ok(handled_count)
}

pub fn parallel_exec_command(options: &AppOptions) -> Result<usize> {
    let thread_size: usize;
    if let Some(threads) = options.jobs {
        thread_size = threads as usize;
        rayon::ThreadPoolBuilder::new().num_threads(threads as usize).build_global().unwrap();
    } else {
        thread_size = num_cpus::get();
    }
    let (out_tx, out_rx) = mpsc::channel::<Vec<String>>();
    let input_reader_buffer = if let Some(buffer_size) = options.buffer_size {
        Some(DESIRED_PIPE_SIZE as usize * thread_size * buffer_size as usize)
    } else {
        None
    };
    if options.verbose_level > 1 {
        debug!("input_reader_buffer: {:?}", input_reader_buffer);
    }
    let output_reader_buffer = input_reader_buffer.clone();
    let (in_tx, in_rx) = mpsc::sync_channel::<Vec<String>>(if let Some(buffer_size) = options.buffer_size {
        // thread_size * buffer_size as usize
        buffer_size as usize
    } else {
        1
    });
    // let (in_tx, in_rx) = mpsc::channel::<Vec<String>>();
    let opt = options.clone();
    let out_handle = thread::spawn(move || -> Result<usize> {
        let mut output_line_count = 0;
        let mut wtr = get_output_writer(&opt.output_file, output_reader_buffer).expect("getting writer failed.");
        for result_vec in out_rx {
            let handled_count = result_vec.len();
            for r in result_vec {
                let _r = writeln!(wtr, "{}", r)?;
            }
            let _r = wtr.flush()?;
            output_line_count += handled_count;
        }
        Ok(output_line_count)
    });

    let opt = options.clone();
    let in_handle = thread::spawn(move || -> Result<usize> {
        let mut input_line_count = 0;
        let mut in_reader = get_input_reader(&opt.input_file, input_reader_buffer)?;
        if let Some(line_size) = opt.lines {
            let line_size = line_size as usize;
            let part_delim = "\n".to_string();
            if opt.verbose_level > 0 {
                debug!("explicit lines mode. line_size: {}", line_size);
            }
            let chunk_size = line_size * thread_size;
            if opt.verbose_level > 0 {
                debug!("before input reader");
            }
            let lines = in_reader.lines();
            for (chunk_n, c) in lines.enumerate().chunks(chunk_size).into_iter().enumerate() {
                let chunk: Result<Vec<String>> = c
                    .map(|(line_n, line)|
                         line.map_err(|e| CommandError::InputReadError(line_n, e))
                    ).collect();
                let chunk = chunk?;
                let chunk_len = chunk.len();
                if opt.verbose_level > 1 {
                    debug!("chunk[{}]: chunk.len(): {}", chunk_n, chunk_len);
                }
                let partition: Vec<Vec<String>> = chunk
                    .chunks(line_size)
                    .map(|c| c.to_vec())
                    .collect();
                let joined_part = partition.join(&part_delim);
                let _r = in_tx.send(joined_part).map_err(|_e| CommandError::SendError(SendChannel::In))?;
                input_line_count += chunk_len;
            }
            if opt.verbose_level > 0 {
                debug!("after input reader");
            }
        } else {
            if opt.verbose_level > 0 {
                debug!("adaptive lines mode.");
            }
            let target_size = DESIRED_PIPE_SIZE as usize;
            let mut current_size = 0;
            let mut partition: Vec<String> = vec![];
            let mut chunk = String::new();
            let mut line_buf: String = String::new();
            let handle_chunks = |chunk: &mut String, partition: &mut Vec<String>, is_last: bool| -> Result<()> {
                // debug!("chunk.len(): {}, part.len(): {}, is_last: {}", chunk.len(), partition.len(), is_last);
                if chunk.len() > 0 {
                    partition.push(chunk.clone());
                    chunk.clear();
                    // debug!("after clear, chunk.len(): {}", chunk.len());
                }
                if is_last || partition.len() >= thread_size {
                    in_tx.send(partition.clone()).map_err(|_e| CommandError::SendError(SendChannel::In))?;
                    partition.clear();
                    Ok(())
                } else {
                    Ok(())
                }
            };
            // let mut line_n = 0;
            loop {
                let read_size = in_reader.read_line(&mut line_buf)?;
                if read_size == 0 {
                    // debug!("last line: line_n:{}", line_n);
                    let _r = handle_chunks(&mut chunk, &mut partition, true)?;
                    break;
                }
                // line_n += 1;
                input_line_count += 1;
                if target_size > (current_size + read_size) {
                    // debug!("line[{}]: line_buf.len(): {}", line_n, line_buf.len());
                    chunk.push_str(&line_buf);
                    current_size += read_size;
                } else {
                    // debug!("line[{}]: handle? line_buf.len(): {}", line_n, line_buf.len());
                    // debug!("[aaa]: target: {}, curr: {}, read: {}", target_size, current_size ,read_size);
                    let _r = handle_chunks(&mut chunk, &mut partition, false)?;
                    current_size = read_size;
                    chunk.push_str(&line_buf);
                }
                line_buf.clear();
            }
            if opt.verbose_level > 0 {
                debug!("after input reader");
            }
        }
        drop(in_tx);
        Ok(input_line_count)
    });
    if options.verbose_level > 0 {
        debug!("before handle");
    }
    let mut total_handled_count = 0;
    for partitions in in_rx {
        let handled_count = run_command_in_parallel(options, partitions, &out_tx)?;
        total_handled_count += handled_count;
    }
    debug!("total_handled_count: {}", total_handled_count);
    drop(out_tx);
    let output_line_count = out_handle.join().map_err(|_e| CommandError::ProcessJoinError)??;
    debug!("output_line_count: {}", output_line_count);
    let input_line_count = in_handle.join().map_err(|_e| CommandError::ProcessJoinError)??;
    debug!("input_line_count: {}", input_line_count);
    Ok(output_line_count)
}
