use panache::format;
use std::{fs, path::Path};

fn normalize(s: &str) -> String {
    s.replace("\r\n", "\n")
}

#[test]
fn golden_cases() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("cases");

    let mut entries: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    let update = std::env::var_os("UPDATE_EXPECTED").is_some();

    for entry in entries {
        let dir = entry.path();
        let input_path = dir.join("input.qmd");
        let expected_path = dir.join("expected.qmd");

        let input = normalize(&fs::read_to_string(&input_path).unwrap());
        let output = format(&input, None);

        // Idempotency: formatting twice should equal once
        let output_twice = format(&output, None);
        similar_asserts::assert_eq!(
            output,
            output_twice,
            "idempotency: {}",
            dir.file_name().unwrap().to_string_lossy()
        );

        if update {
            fs::write(&expected_path, &output).unwrap();
            continue;
        }

        let expected = fs::read_to_string(&expected_path)
            .map(|s| normalize(&s))
            .unwrap_or_else(|_| input.clone());

        similar_asserts::assert_eq!(
            expected,
            output,
            "case: {}",
            dir.file_name().unwrap().to_string_lossy()
        );
    }
}
