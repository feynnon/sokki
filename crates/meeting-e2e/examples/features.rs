//! Prints which optional features a model supports.
use transcribe_cpp::{Feature, Model, ModelOptions};
fn main() {
    let _ = transcribe_cpp::init_backends_default();
    for path in std::env::args().skip(1) {
        let m = Model::load_with(&path, &ModelOptions::default()).unwrap();
        let f = [
            ("Vocabulary", Feature::Vocabulary),
            ("ContextPrompt", Feature::ContextPrompt),
            ("Pnc", Feature::Pnc),
            ("Itn", Feature::Itn),
            ("Instruct", Feature::Instruct),
        ];
        let on: Vec<_> = f
            .iter()
            .filter(|(_, x)| m.supports(*x))
            .map(|(n, _)| *n)
            .collect();
        println!("{} -> {:?}", path.rsplit('/').next().unwrap(), on);
    }
}
