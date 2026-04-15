use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};

#[derive(Clone)]
struct Row {
    function: String,
    input_hash: String,
    output_hash: String,
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: tracehash-compare <left.tsv> <right.tsv>");
        std::process::exit(2);
    }

    let left = read_rows(&args[1]).unwrap_or_else(|err| die(&format!("{}: {}", args[1], err)));
    let right = read_rows(&args[2]).unwrap_or_else(|err| die(&format!("{}: {}", args[2], err)));

    let mut status = 0;
    status |= compare_counts(&left, &right);
    status |= compare_pairs(&left, &right);

    if status == 0 {
        println!(
            "tracehash: traces match for {} left rows and {} right rows",
            left.len(),
            right.len()
        );
    }
    std::process::exit(status);
}

fn die(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(2);
}

fn read_rows(path: &str) -> std::io::Result<Vec<Row>> {
    let file = File::open(path)?;
    let mut rows = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 12 {
            continue;
        }
        rows.push(Row {
            function: cols[4].to_string(),
            input_hash: cols[5].to_string(),
            output_hash: cols[6].to_string(),
        });
    }
    Ok(rows)
}

fn compare_counts(left: &[Row], right: &[Row]) -> i32 {
    let mut lcounts = BTreeMap::<&str, usize>::new();
    let mut rcounts = BTreeMap::<&str, usize>::new();
    for row in left {
        *lcounts.entry(&row.function).or_default() += 1;
    }
    for row in right {
        *rcounts.entry(&row.function).or_default() += 1;
    }

    let functions: BTreeSet<&str> = lcounts.keys().chain(rcounts.keys()).copied().collect();
    let mut diffs = 0;
    for function in functions {
        let lc = lcounts.get(function).copied().unwrap_or(0);
        let rc = rcounts.get(function).copied().unwrap_or(0);
        if lc != rc {
            if diffs == 0 {
                println!("count differences:");
            }
            println!("  {function}: left={lc} right={rc}");
            diffs += 1;
        }
    }
    (diffs != 0) as i32
}

fn compare_pairs(left: &[Row], right: &[Row]) -> i32 {
    let lmap = pair_map(left);
    let rmap = pair_map(right);
    let keys: BTreeSet<(&str, &str)> = lmap
        .keys()
        .map(|(f, i)| (f.as_str(), i.as_str()))
        .chain(rmap.keys().map(|(f, i)| (f.as_str(), i.as_str())))
        .collect();

    let mut missing = 0;
    let mut mismatched = 0;
    let mut by_function = BTreeMap::<String, (usize, usize)>::new();
    for (function, input_hash) in keys {
        let key = (function.to_string(), input_hash.to_string());
        match (lmap.get(&key), rmap.get(&key)) {
            (Some(left_outputs), Some(right_outputs)) if left_outputs != right_outputs => {
                by_function.entry(function.to_string()).or_default().1 += 1;
                if mismatched < 20 {
                    println!(
                        "output mismatch: {function} input={input_hash} left={:?} right={:?}",
                        left_outputs, right_outputs
                    );
                }
                mismatched += 1;
            }
            (Some(_), None) => {
                by_function.entry(function.to_string()).or_default().0 += 1;
                if missing < 20 {
                    println!("missing on right: {function} input={input_hash}");
                }
                missing += 1;
            }
            (None, Some(_)) => {
                by_function.entry(function.to_string()).or_default().0 += 1;
                if missing < 20 {
                    println!("missing on left: {function} input={input_hash}");
                }
                missing += 1;
            }
            _ => {}
        }
    }

    if missing > 20 {
        println!("... {} more missing inputs", missing - 20);
    }
    if mismatched > 20 {
        println!("... {} more output mismatches", mismatched - 20);
    }
    if !by_function.is_empty() {
        println!("pair differences by function:");
        for (function, (missing_count, mismatch_count)) in by_function {
            println!("  {function}: missing_inputs={missing_count} output_mismatches={mismatch_count}");
        }
    }
    ((missing + mismatched) != 0) as i32
}

fn pair_map(rows: &[Row]) -> HashMap<(String, String), BTreeMap<String, usize>> {
    let mut map = HashMap::<(String, String), BTreeMap<String, usize>>::new();
    for row in rows {
        let outputs = map
            .entry((row.function.clone(), row.input_hash.clone()))
            .or_default();
        *outputs.entry(row.output_hash.clone()).or_default() += 1;
    }
    map
}
