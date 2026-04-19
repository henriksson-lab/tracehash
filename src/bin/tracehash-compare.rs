use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};

#[derive(Clone)]
struct Row {
    function: String,
    input_hash: String,
    output_hash: String,
    values: Option<String>,
}

struct Options {
    left_path: String,
    right_path: String,
    left_label: String,
    right_label: String,
    only: BTreeSet<String>,
    skip: BTreeSet<String>,
    first_limit: usize,
    summary_only: bool,
}

fn main() {
    let options = parse_args(env::args().skip(1).collect()).unwrap_or_else(|err| die(&err));

    let mut left = read_rows(&options.left_path)
        .unwrap_or_else(|err| die(&format!("{}: {}", options.left_path, err)));
    let mut right = read_rows(&options.right_path)
        .unwrap_or_else(|err| die(&format!("{}: {}", options.right_path, err)));
    filter_rows(&mut left, &options);
    filter_rows(&mut right, &options);

    let mut status = 0;
    status |= compare_counts(&left, &right, &options.left_label, &options.right_label);
    status |= compare_occurrences(
        &left,
        &right,
        options.first_limit,
        &options.left_label,
        &options.right_label,
        options.summary_only,
    );
    status |= compare_pairs(
        &left,
        &right,
        &options.left_label,
        &options.right_label,
        options.summary_only,
    );

    if status == 0 {
        println!(
            "tracehash: traces match for {} {} rows and {} {} rows",
            left.len(),
            options.left_label,
            right.len(),
            options.right_label
        );
    }
    std::process::exit(status);
}

fn parse_args(args: Vec<String>) -> Result<Options, String> {
    let mut only = BTreeSet::new();
    let mut skip = BTreeSet::new();
    let mut first_limit = 20usize;
    let mut left_label = "left".to_string();
    let mut right_label = "right".to_string();
    let mut summary_only = false;
    let mut paths = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--only" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "--only requires a comma-separated function list".to_string())?;
                insert_function_list(&mut only, value);
            }
            "--skip" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "--skip requires a comma-separated function list".to_string())?;
                insert_function_list(&mut skip, value);
            }
            "--first" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "--first requires a mismatch count".to_string())?;
                first_limit = value
                    .parse()
                    .map_err(|_| format!("invalid --first value: {value}"))?;
            }
            "--left-label" => {
                i += 1;
                left_label = args
                    .get(i)
                    .ok_or_else(|| "--left-label requires a label".to_string())?
                    .to_string();
            }
            "--right-label" => {
                i += 1;
                right_label = args
                    .get(i)
                    .ok_or_else(|| "--right-label requires a label".to_string())?
                    .to_string();
            }
            "--summary-only" => {
                summary_only = true;
            }
            "-h" | "--help" => return Err(usage().to_string()),
            arg if arg.starts_with('-') => {
                return Err(format!("unknown option: {arg}\n{}", usage()))
            }
            path => paths.push(path.to_string()),
        }
        i += 1;
    }

    if paths.len() != 2 {
        return Err(usage().to_string());
    }
    Ok(Options {
        left_path: paths.remove(0),
        right_path: paths.remove(0),
        left_label,
        right_label,
        only,
        skip,
        first_limit,
        summary_only,
    })
}

fn usage() -> &'static str {
    "usage: tracehash-compare [--only f1,f2] [--skip f3,f4] [--first N] [--left-label NAME] [--right-label NAME] [--summary-only] <left.tsv> <right.tsv>"
}

fn insert_function_list(out: &mut BTreeSet<String>, value: &str) {
    for function in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        out.insert(function.to_string());
    }
}

fn die(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(2);
}

fn filter_rows(rows: &mut Vec<Row>, options: &Options) {
    rows.retain(|row| {
        (options.only.is_empty() || options.only.contains(&row.function))
            && !options.skip.contains(&row.function)
    });
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
        let values = match cols.len() {
            12 => None,
            13 => {
                // Could be old-format `values` column, or new-format
                // `deep_seq` column. `deep_seq` is "-" or decimal digits.
                let last = cols[12];
                if last == "-" || (!last.is_empty() && last.bytes().all(|b| b.is_ascii_digit())) {
                    None
                } else {
                    Some(last.to_string())
                }
            }
            _ => cols.get(13).map(|s| s.to_string()),
        };
        rows.push(Row {
            function: cols[4].to_string(),
            input_hash: cols[5].to_string(),
            output_hash: cols[6].to_string(),
            values,
        });
    }
    Ok(rows)
}

fn compare_counts(left: &[Row], right: &[Row], left_label: &str, right_label: &str) -> i32 {
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
            println!("  {function}: {left_label}={lc} {right_label}={rc}");
            diffs += 1;
        }
    }
    (diffs != 0) as i32
}

fn compare_occurrences(
    left: &[Row],
    right: &[Row],
    first_limit: usize,
    left_label: &str,
    right_label: &str,
    summary_only: bool,
) -> i32 {
    let lmap = occurrence_map(left);
    let rmap = occurrence_map(right);
    let keys: BTreeSet<(String, String)> = lmap.keys().chain(rmap.keys()).cloned().collect();
    let mut differences = 0usize;
    let mut printed = 0usize;

    for key in keys {
        let left_rows = lmap.get(&key).map(Vec::as_slice).unwrap_or(&[]);
        let right_rows = rmap.get(&key).map(Vec::as_slice).unwrap_or(&[]);
        let max_len = left_rows.len().max(right_rows.len());
        for occurrence in 0..max_len {
            match (left_rows.get(occurrence), right_rows.get(occurrence)) {
                (Some(left_row), Some(right_row))
                    if left_row.output_hash != right_row.output_hash =>
                {
                    if !summary_only && printed < first_limit {
                        print_occurrence_mismatch(
                            "output mismatch",
                            &key.0,
                            &key.1,
                            occurrence,
                            Some(left_row),
                            Some(right_row),
                            left_label,
                            right_label,
                        );
                        printed += 1;
                    }
                    differences += 1;
                }
                (Some(left_row), None) => {
                    if !summary_only && printed < first_limit {
                        print_occurrence_mismatch(
                            &format!("missing on {right_label}"),
                            &key.0,
                            &key.1,
                            occurrence,
                            Some(left_row),
                            None,
                            left_label,
                            right_label,
                        );
                        printed += 1;
                    }
                    differences += 1;
                }
                (None, Some(right_row)) => {
                    if !summary_only && printed < first_limit {
                        print_occurrence_mismatch(
                            &format!("missing on {left_label}"),
                            &key.0,
                            &key.1,
                            occurrence,
                            None,
                            Some(right_row),
                            left_label,
                            right_label,
                        );
                        printed += 1;
                    }
                    differences += 1;
                }
                _ => {}
            }
        }
    }

    if differences != 0 {
        if !summary_only && differences > printed {
            println!(
                "... {} more occurrence differences (raise --first to print more)",
                differences - printed
            );
        }
        println!("occurrence differences: {differences}");
    }
    (differences != 0) as i32
}

fn occurrence_map(rows: &[Row]) -> BTreeMap<(String, String), Vec<&Row>> {
    let mut map = BTreeMap::<(String, String), Vec<&Row>>::new();
    for row in rows {
        map.entry((row.function.clone(), row.input_hash.clone()))
            .or_default()
            .push(row);
    }
    map
}

fn print_occurrence_mismatch(
    label: &str,
    function: &str,
    input_hash: &str,
    occurrence: usize,
    left: Option<&Row>,
    right: Option<&Row>,
    left_label: &str,
    right_label: &str,
) {
    println!("{label}: {function} input={input_hash} occurrence={occurrence}");
    if let Some(row) = left {
        println!("  {left_label} output={}", row.output_hash);
        if let Some(values) = &row.values {
            println!("  {left_label} values={values}");
        }
    }
    if let Some(row) = right {
        println!("  {right_label} output={}", row.output_hash);
        if let Some(values) = &row.values {
            println!("  {right_label} values={values}");
        }
    }
}

fn compare_pairs(
    left: &[Row],
    right: &[Row],
    left_label: &str,
    right_label: &str,
    summary_only: bool,
) -> i32 {
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
                if !summary_only && mismatched < 20 {
                    println!(
                        "output mismatch: {function} input={input_hash} {left_label}={:?} {right_label}={:?}",
                        left_outputs, right_outputs
                    );
                }
                mismatched += 1;
            }
            (Some(_), None) => {
                by_function.entry(function.to_string()).or_default().0 += 1;
                if !summary_only && missing < 20 {
                    println!("missing on {right_label}: {function} input={input_hash}");
                }
                missing += 1;
            }
            (None, Some(_)) => {
                by_function.entry(function.to_string()).or_default().0 += 1;
                if !summary_only && missing < 20 {
                    println!("missing on {left_label}: {function} input={input_hash}");
                }
                missing += 1;
            }
            _ => {}
        }
    }

    if !summary_only && missing > 20 {
        println!("... {} more missing inputs", missing - 20);
    }
    if !summary_only && mismatched > 20 {
        println!("... {} more output mismatches", mismatched - 20);
    }
    if !by_function.is_empty() {
        println!("pair differences by function:");
        for (function, (missing_count, mismatch_count)) in by_function {
            println!(
                "  {function}: missing_inputs={missing_count} output_mismatches={mismatch_count}"
            );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_filters_labels_and_summary_mode() {
        let options = parse_args(vec![
            "--only".into(),
            "foo,bar".into(),
            "--skip".into(),
            "baz".into(),
            "--first".into(),
            "7".into(),
            "--left-label".into(),
            "rust".into(),
            "--right-label".into(),
            "c".into(),
            "--summary-only".into(),
            "left.tsv".into(),
            "right.tsv".into(),
        ])
        .unwrap();

        assert_eq!(options.left_path, "left.tsv");
        assert_eq!(options.right_path, "right.tsv");
        assert_eq!(options.left_label, "rust");
        assert_eq!(options.right_label, "c");
        assert_eq!(options.first_limit, 7);
        assert!(options.summary_only);
        assert!(options.only.contains("foo"));
        assert!(options.only.contains("bar"));
        assert!(options.skip.contains("baz"));
    }

    #[test]
    fn filters_rows_by_only_and_skip_lists() {
        let options = parse_args(vec![
            "--only".into(),
            "keep,drop".into(),
            "--skip".into(),
            "drop".into(),
            "left.tsv".into(),
            "right.tsv".into(),
        ])
        .unwrap();
        let mut rows = vec![test_row("keep"), test_row("drop"), test_row("other")];

        filter_rows(&mut rows, &options);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].function, "keep");
    }

    fn test_row(function: &str) -> Row {
        Row {
            function: function.to_string(),
            input_hash: "input".to_string(),
            output_hash: "output".to_string(),
            values: None,
        }
    }
}
