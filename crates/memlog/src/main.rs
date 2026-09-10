use ped_memlog::{
    CSV_HEADER, csv_row, find_process, output_needs_header, read_smaps, read_status, read_uptime,
};
use std::env;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct Options {
    interval: u64,
    output: Option<PathBuf>,
    process_name: String,
}

fn usage() {
    eprintln!("Usage: ped-memlog [OPTIONS] [interval_seconds] [output.csv]");
    eprintln!("  --interval SECONDS       Sampling interval (default: 5)");
    eprintln!("  --output PATH            Append CSV to PATH (default: stdout)");
    eprintln!("  --process-name NAME      Process comm name (default: ped)");
    eprintln!("  -h, --help               Show this help");
}

fn parse_options() -> Result<Options, String> {
    let mut interval = 5;
    let mut output = None;
    let mut process_name = String::from("ped");
    let mut positional = Vec::new();
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                usage();
                std::process::exit(0);
            }
            "--interval" => {
                interval = next_value(&mut args, "--interval")?
                    .parse()
                    .map_err(|_| "invalid interval".to_string())?
            }
            "--output" => output = Some(PathBuf::from(next_value(&mut args, "--output")?)),
            "--process-name" | "--name" => process_name = next_value(&mut args, "--process-name")?,
            value if value.starts_with("--") => return Err(format!("unknown option: {value}")),
            value => positional.push(value.to_string()),
        }
    }
    if let Some(value) = positional.first() {
        interval = value.parse().map_err(|_| "invalid interval".to_string())?;
    }
    if let Some(value) = positional.get(1) {
        output = Some(PathBuf::from(value));
    }
    if interval == 0 {
        return Err("interval must be greater than zero".to_string());
    }
    if process_name.is_empty() || process_name.contains(['\r', '\n']) {
        return Err("process name must not be empty or contain a newline".to_string());
    }
    Ok(Options {
        interval,
        output,
        process_name,
    })
}

fn next_value(args: &mut impl Iterator<Item = String>, option: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{option} requires a value"))
}

fn unix_timestamp() -> io::Result<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "system clock is before Unix epoch"))
}

fn main() -> io::Result<()> {
    let options = parse_options().map_err(|error| {
        usage();
        io::Error::new(io::ErrorKind::InvalidInput, error)
    })?;
    let mut file = match options.output.as_deref() {
        Some(path) => {
            let needs_header = output_needs_header(path)?;
            let mut file = OpenOptions::new().create(true).append(true).open(path)?;
            if needs_header {
                writeln!(file, "{CSV_HEADER}")?;
                file.flush()?;
            }
            Some(file)
        }
        None => None,
    };
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    let mut stdout_header_written = false;
    let mut previous_pid = None;
    loop {
        match find_process(&options.process_name)? {
            None => {
                if previous_pid.is_some() {
                    eprintln!("{} process exited", options.process_name);
                    previous_pid = None;
                } else {
                    eprintln!("{} process not found", options.process_name);
                }
            }
            Some(pid) => {
                if previous_pid != Some(pid) {
                    match previous_pid {
                        Some(old_pid) => {
                            eprintln!("{} PID changed: {old_pid} -> {pid}", options.process_name)
                        }
                        None => eprintln!("{} found: pid={pid}", options.process_name),
                    }
                    previous_pid = Some(pid);
                }
                match sample(pid) {
                    Ok(row) => {
                        if let Some(file) = file.as_mut() {
                            writeln!(file, "{row}")?;
                            file.flush()?;
                        } else {
                            if !stdout_header_written {
                                writeln!(stdout, "{CSV_HEADER}")?;
                                stdout_header_written = true;
                            }
                            writeln!(stdout, "{row}")?;
                            stdout.flush()?;
                        }
                    }
                    Err(error) => eprintln!("sample pid={pid} skipped: {error}"),
                }
            }
        }
        thread::sleep(Duration::from_secs(options.interval));
    }
}

fn sample(pid: u32) -> io::Result<String> {
    let (vm_rss_kb, vm_size_kb) = read_status(pid)?;
    let totals = read_smaps(pid)?;
    Ok(csv_row(
        unix_timestamp()?,
        read_uptime()?,
        pid,
        vm_rss_kb,
        vm_size_kb,
        &totals,
    ))
}
