# ped-memlog

`ped-memlog` is a dependency-free Rust CLI that periodically records the
memory usage of a named Linux process. It is intended for long-running PED
experiments on Kindle and does not require `ps`, `pidof`, `pgrep`, or a
`procfs` crate.

## Usage

```sh
./ped-memlog
./ped-memlog --interval 5 --output /mnt/us/ped-memory.csv
./ped-memlog --process-name my-process --interval 1 --output memory.csv
```

The default process name is `ped`, and the default interval is 5 seconds.
The process name is matched against `/proc/<pid>/comm` and can be changed
while starting the logger with `--process-name` (or `--name`). The two
positional arguments `interval_seconds` and `output.csv` are also accepted.

The logger rescans `/proc` on every sample, so it follows a restarted process.
It keeps running when the process is absent and skips samples affected by a
process exiting while `/proc` is being read. A file output is appended to and
gets a header only when it is new or empty. Standard output includes the CSV
header once before the first valid row.

## CSV format

The output is UTF-8 text with one comma-separated record per successful sample.
There is no quoting because all fields are numeric:

```csv
timestamp_unix,uptime_s,pid,vm_rss_kb,vm_size_kb,rss_kb,pss_kb,anonymous_kb,private_dirty_kb,private_clean_kb,shared_dirty_kb,shared_clean_kb
1788992400,3600.25,1234,194120,663376,194116,187532,180421,178932,1024,2048,7168
```

| Column | Meaning |
|---|---|
| `timestamp_unix` | Wall-clock Unix timestamp in whole seconds. |
| `uptime_s` | `/proc/uptime` seconds since boot, with two decimal places. |
| `pid` | PID used for this sample. |
| `vm_rss_kb` | `VmRSS` from `/proc/<pid>/status`, in KiB. |
| `vm_size_kb` | `VmSize` from `/proc/<pid>/status`, in KiB. |
| `rss_kb` | Sum of `Rss` fields in `/proc/<pid>/smaps`, in KiB. |
| `pss_kb` | Sum of `Pss` fields in `smaps`, in KiB. |
| `anonymous_kb` | Sum of `Anonymous` fields in `smaps`, in KiB. |
| `private_dirty_kb` | Sum of `Private_Dirty` fields in `smaps`, in KiB. |
| `private_clean_kb` | Sum of `Private_Clean` fields in `smaps`, in KiB. |
| `shared_dirty_kb` | Sum of `Shared_Dirty` fields in `smaps`, in KiB. |
| `shared_clean_kb` | Sum of `Shared_Clean` fields in `smaps`, in KiB. |

`vm_rss_kb` and `rss_kb` are read at different times and may differ slightly.
`timestamp_unix` is useful for wall-clock correlation; `uptime_s` is the
stable time axis for long experiments when the system clock changes.

## Build

From the repository root:

```sh
cargo check --target armv7-unknown-linux-musleabihf -p ped-memlog
cargo build --release --target armv7-unknown-linux-musleabihf -p ped-memlog
```
