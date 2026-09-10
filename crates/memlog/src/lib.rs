use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

pub const CSV_HEADER: &str = "timestamp_unix,uptime_s,pid,vm_rss_kb,vm_size_kb,rss_kb,pss_kb,anonymous_kb,private_dirty_kb,private_clean_kb,shared_dirty_kb,shared_clean_kb";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct SmapsTotals {
    pub rss_kb: u64,
    pub pss_kb: u64,
    pub anonymous_kb: u64,
    pub private_dirty_kb: u64,
    pub private_clean_kb: u64,
    pub shared_dirty_kb: u64,
    pub shared_clean_kb: u64,
}

impl SmapsTotals {
    pub fn parse<R: BufRead>(reader: R) -> io::Result<Self> {
        let mut totals = Self::default();
        for line in reader.lines() {
            let line = line?;
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.split_whitespace().next().unwrap_or("");
            let Ok(value) = value.parse::<u64>() else {
                continue;
            };
            match key {
                "Rss" => totals.rss_kb += value,
                "Pss" => totals.pss_kb += value,
                "Anonymous" => totals.anonymous_kb += value,
                "Private_Dirty" => totals.private_dirty_kb += value,
                "Private_Clean" => totals.private_clean_kb += value,
                "Shared_Dirty" => totals.shared_dirty_kb += value,
                "Shared_Clean" => totals.shared_clean_kb += value,
                _ => {}
            }
        }
        Ok(totals)
    }
}

pub fn find_process(name: &str) -> io::Result<Option<u32>> {
    let mut found: Option<u32> = None;
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        let file_name = entry.file_name();
        let pid = file_name
            .to_str()
            .and_then(|value| value.parse::<u32>().ok());
        let Some(pid) = pid else {
            continue;
        };
        let comm = match std::fs::read_to_string(entry.path().join("comm")) {
            Ok(comm) => comm,
            Err(_) => continue,
        };
        if comm.trim_end_matches(['\r', '\n']) == name {
            found = Some(found.map_or(pid, |current| current.min(pid)));
        }
    }
    Ok(found)
}

pub fn read_status(pid: u32) -> io::Result<(u64, u64)> {
    let file = File::open(format!("/proc/{pid}/status"))?;
    let reader = BufReader::new(file);
    let mut vm_rss_kb = None;
    let mut vm_size_kb = None;
    for line in reader.lines() {
        let line = line?;
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.split_whitespace().next().unwrap_or("");
        match key {
            "VmRSS" => vm_rss_kb = value.parse().ok(),
            "VmSize" => vm_size_kb = value.parse().ok(),
            _ => {}
        }
    }
    match (vm_rss_kb, vm_size_kb) {
        (Some(rss), Some(size)) => Ok((rss, size)),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing VmRSS or VmSize",
        )),
    }
}

pub fn read_smaps(pid: u32) -> io::Result<SmapsTotals> {
    let file = File::open(format!("/proc/{pid}/smaps"))?;
    SmapsTotals::parse(BufReader::new(file))
}

pub fn read_uptime() -> io::Result<f64> {
    let uptime = std::fs::read_to_string("/proc/uptime")?;
    uptime
        .split_whitespace()
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing uptime"))
        .and_then(|value| {
            value
                .parse()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid uptime"))
        })
}

pub fn csv_row(
    timestamp_unix: i64,
    uptime_s: f64,
    pid: u32,
    vm_rss_kb: u64,
    vm_size_kb: u64,
    totals: &SmapsTotals,
) -> String {
    format!(
        "{timestamp_unix},{uptime_s:.2},{pid},{vm_rss_kb},{vm_size_kb},{},{},{},{},{},{},{}",
        totals.rss_kb,
        totals.pss_kb,
        totals.anonymous_kb,
        totals.private_dirty_kb,
        totals.private_clean_kb,
        totals.shared_dirty_kb,
        totals.shared_clean_kb,
    )
}

pub fn output_needs_header(path: &Path) -> io::Result<bool> {
    Ok(!path.exists() || std::fs::metadata(path)?.len() == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parses_smaps_totals_and_ignores_other_fields() {
        let input = b"00400000-00401000 r--p\n\
                      Rss:                  12 kB\n\
                      Pss:                   7 kB\n\
                      Anonymous:             3 kB\n\
                      Private_Dirty:         2 kB\n\
                      Private_Clean:         1 kB\n\
                      Shared_Dirty:          4 kB\n\
                      Shared_Clean:          5 kB\n\
                      Referenced:            99 kB\n\
                      Rss:                   8 kB\n\
                      Pss:                   6 kB\n\
                      Anonymous:             2 kB\n\
                      Private_Dirty:         1 kB\n\
                      Private_Clean:         3 kB\n\
                      Shared_Dirty:          2 kB\n\
                      Shared_Clean:          1 kB\n";
        assert_eq!(
            SmapsTotals::parse(Cursor::new(input)).unwrap(),
            SmapsTotals {
                rss_kb: 20,
                pss_kb: 13,
                anonymous_kb: 5,
                private_dirty_kb: 3,
                private_clean_kb: 4,
                shared_dirty_kb: 6,
                shared_clean_kb: 6,
            }
        );
    }

    #[test]
    fn formats_a_timestamped_csv_row() {
        let row = csv_row(
            1_788_992_400,
            3600.25,
            1234,
            194120,
            663376,
            &SmapsTotals::default(),
        );
        assert_eq!(row, "1788992400,3600.25,1234,194120,663376,0,0,0,0,0,0,0");
    }
}
