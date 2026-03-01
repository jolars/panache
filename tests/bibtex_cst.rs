use std::fs;
use std::path::Path;

use panache::bibtex::parse_bibtex_cst;

fn run_case(input_path: &Path, snapshot_path: &Path) {
    let input = fs::read_to_string(input_path).unwrap();
    let node = parse_bibtex_cst(&input);
    let cst_output = format!("{:#?}\n", node);

    if std::env::var_os("UPDATE_BIBTEX_CST").is_some() {
        fs::write(snapshot_path, cst_output).unwrap();
    } else {
        let expected = fs::read_to_string(snapshot_path).unwrap();
        assert_eq!(expected, cst_output);
    }

    assert_eq!(node.text().to_string(), input);
}

#[test]
fn parse_generated_bibtex_entries() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    run_case(
        &root.join("tests/bibtex_samples/bibtex.bib"),
        &root.join("tests/bibtex_samples/bibtex.cst"),
    );
}

#[test]
fn parse_averroes_fixture() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    run_case(
        &root.join("tests/bibtex_samples/averroes.bib"),
        &root.join("tests/bibtex_samples/averroes.cst"),
    );
}

#[test]
fn parse_generated_biblatex_entries() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    run_case(
        &root.join("tests/bibtex_samples/biblatex.bib"),
        &root.join("tests/bibtex_samples/biblatex.cst"),
    );
}
