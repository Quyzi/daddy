//! Built-in rule packs, embedded into the binary at compile time so
//! `brain` works from any working directory without needing the source
//! repository's `packs/` folder to be present at runtime. A pack name
//! that isn't one of the built-ins is treated as a filesystem path to a
//! custom pack file instead.

use anyhow::{Context, Result};
use brain_index::{CompiledPack, RulePack};
use std::path::Path;

const GENERIC_PACK_TOML: &str = include_str!("../../../packs/generic.toml");
const DND5E_PACK_TOML: &str = include_str!("../../../packs/dnd5e.toml");

/// Loads and compiles a rule pack by name (`"generic"`, `"dnd5e"`, or a
/// path to a custom `.toml` file), appending the built-in generic pack's
/// catch-all rules after it unless `name` already *is* `"generic"` — see
/// [`brain_index::CompiledPack::load_with_fallback`]'s docs for why.
pub fn load_pack(name: &str) -> Result<CompiledPack> {
    let mut pack = match name {
        "generic" => RulePack::parse(GENERIC_PACK_TOML).context("parsing built-in generic pack")?,
        "dnd5e" => RulePack::parse(DND5E_PACK_TOML).context("parsing built-in dnd5e pack")?,
        path => RulePack::load(Path::new(path))
            .with_context(|| format!("loading custom rule pack {path:?}"))?,
    };
    if name != "generic" {
        let generic = RulePack::parse(GENERIC_PACK_TOML).context("parsing built-in generic pack")?;
        pack.entities.extend(generic.entities);
    }
    CompiledPack::compile(&pack).context("compiling rule pack")
}
