# parallel

Tool for running specified command in parallel against input.

## Usage
```
USAGE:
    parallel [FLAGS] [OPTIONS] [commands]...

FLAGS:
    -D, --dry-run    Dry run
    -h, --help       Prints help information
    -V, --version    Prints version information
    -v               Sets the level of verbosity (-v, -vv, -vvv, etc.)

OPTIONS:
    -B, --buffer-size <BUFFER_SIZE>    Specify number of size for buffer
    -i, --input-file <FILE>            Specify input file. If not specified, STDIN will be used
    -j, --jobs <JOBS>                  Specify number of jobs
    -L, --lines <LINE>                 Specify number of lines each job handles in batches
    -o, --output-file <FILE>           Specify output file. If not specified, write to STDOUT

ARGS:
    <commands>...    Commands to run in parallel
```

## Example

Running `jq` command in parallel.

    $ cat large.json | parallel -B 100 -- jq -r '.request."status"' | sort | uniq -c
