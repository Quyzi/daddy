use brain_index::{CompiledPack, RulePack};
use std::path::Path;

fn main() {
    for name in ["generic", "dnd5e"] {
        let path = format!("packs/{name}.toml");
        let pack = RulePack::load(Path::new(&path)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let compiled = CompiledPack::compile(&pack).unwrap_or_else(|e| panic!("{name} compile: {e}"));
        println!("{name}: {} rules", compiled.rules.len());
        for r in &compiled.rules {
            println!("  - kind={} atomic={} requires={} fields={:?}", r.kind, r.atomic, r.requires.len(), r.fields);
        }
    }

    let with_fallback = CompiledPack::load_with_fallback(Path::new("packs"), "dnd5e").unwrap();
    println!("dnd5e+fallback: {} rules", with_fallback.rules.len());
}
