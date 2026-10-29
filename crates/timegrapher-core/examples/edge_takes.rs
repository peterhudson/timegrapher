//! Write the synthetic edge takes (`timegrapher_core::edge_takes`) under
//! the folder given, laid out as a corpus.
fn main() {
    let Some(root) = std::env::args_os().nth(1) else {
        eprintln!("usage: edge_takes DIR");
        std::process::exit(2);
    };
    match timegrapher_core::edge_takes::write_all(std::path::Path::new(&root)) {
        Ok(takes) => eprintln!("{} edge takes written", takes.len()),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
