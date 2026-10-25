//! The bundler: one ES module that behaves like the unbundled program.
//!
//! What "behaves like" means, and what is refused instead, is the contract in
//! `SEMANTICS.md` next to this crate's manifest. Nothing here resolves an
//! import: the module graph comes from opal-core.
//!
//! A build runs in four phases, timed separately:
//!
//! 1. **parse**: read each module the graph lists, check it is the file the
//!    graph hashed, classify it as Node would, parse it, compile TypeScript
//!    and JSX, and record what linking needs.
//! 2. **link**: resolve every import and re-export, order the modules, find
//!    the lazy ones, and name every top-level binding.
//! 3. **print**: rewrite each module and print the bundle with its source
//!    map.
//! 4. **write**: put both files in the out-dir, each replaced atomically.

pub mod analyze;
pub mod cjs_lexer;
pub mod define;
pub mod diagnostic;
pub mod emit;
pub mod glob;
pub mod link;
pub mod load;
pub mod options;
pub mod runtime;
pub mod shake;
pub mod tsconfig;

#[cfg(feature = "oracle")]
pub mod generate;
#[cfg(feature = "oracle")]
pub mod oracle;

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use oxc_allocator::Allocator;

use opal_core::graph::{ModuleGraph, ModuleId};

pub use diagnostic::{Code, Diagnostic};
pub use options::{BuildOptions, Define, Target};

use crate::analyze::Info;
use crate::define::Defines;
use crate::link::{LinkModule, Role};
use crate::load::{Format, Language, PackageTypes, Parsed, Source};
use crate::tsconfig::TsConfigs;

/// How long each phase took.
#[derive(Clone, Copy, Debug, Default)]
pub struct Timings {
    pub parse: Duration,
    pub link: Duration,
    pub print: Duration,
    pub write: Duration,
}

pub struct BuildOutput {
    /// `<entry stem>.mjs`.
    pub file_name: String,
    pub code: String,
    pub map: String,
    /// Modules in the bundle.
    pub modules: usize,
    pub timings: Timings,
}

/// Bundles the graph's entry, for an out-dir at `outdir` (absolute). Nothing
/// is written; see [`write`].
pub fn build(
    graph: &ModuleGraph,
    outdir: &Path,
    options: &BuildOptions,
) -> Result<BuildOutput, Diagnostic> {
    let started = Instant::now();
    let root = graph.root().as_path().to_path_buf();
    let mut packages = PackageTypes::default();
    let mut tsconfigs = TsConfigs::new(&root, &options.jsx_import_source);

    // Every module the graph lists, then the bundler's own helpers.
    let mut sources: Vec<Source> = Vec::with_capacity(graph.len() + runtime::VIRTUAL_MODULES.len());
    for module in graph.modules() {
        let id = graph
            .id_of(&module.path)
            .expect("every module in the graph has an id");
        sources.push(load::read(graph, id, &mut packages)?);
    }
    let mut virtual_ids = HashMap::new();
    for (name, text) in runtime::VIRTUAL_MODULES {
        virtual_ids.insert(name.to_string(), sources.len());
        sources.push(Source {
            path: format!("{}{name}.js", link::VIRTUAL_PREFIX),
            absolute: root.join(".opal-virtual").join(format!("{name}.js")),
            format: Format::Esm,
            text: text.to_string(),
            language: Language::JavaScript,
        });
    }

    let allocators: Vec<Allocator> = sources.iter().map(|_| Allocator::default()).collect();
    let mut parsed: Vec<Option<Parsed<'_>>> = Vec::with_capacity(sources.len());
    let mut infos: Vec<Info> = Vec::with_capacity(sources.len());
    let mut lexed: Vec<Option<(Vec<String>, Vec<String>)>> = Vec::with_capacity(sources.len());
    for (source, allocator) in sources.iter().zip(&allocators) {
        if matches!(source.format, Format::Json | Format::Opaque) {
            parsed.push(None);
            infos.push(Info::opaque(source));
            lexed.push(None);
            continue;
        }
        let settings = load::settings_for(&mut tsconfigs, source, &options.jsx_import_source)?;
        let module = load::parse(allocator, source, &settings)?;
        let info = analyze::analyze(source, &module)?;
        lexed.push(match source.format {
            Format::Cjs => Some(cjs_lexer::exports(&source.text)),
            _ => None,
        });
        infos.push(info);
        parsed.push(Some(module));
    }
    let parse_time = started.elapsed();

    let started = Instant::now();
    let empty_scoping = oxc_semantic::Scoping::default();
    let empty_lines = diagnostic::LineIndex::new("");
    let link_modules: Vec<LinkModule<'_>> = (0..sources.len())
        .map(|index| LinkModule {
            info: &infos[index],
            scoping: parsed[index]
                .as_ref()
                .map_or(&empty_scoping, |p| &p.scoping),
            lines: parsed[index].as_ref().map_or(&empty_lines, |p| &p.lines),
            text: parsed[index].as_ref().map_or("", |p| p.text),
            graph_id: (index < graph.len()).then(|| module_id(graph, index)),
            cjs_lexed: lexed[index].clone(),
            side_effect_free: sources[index].format == Format::Esm
                && index < graph.len()
                && packages.side_effect_free(&sources[index].absolute),
        })
        .collect();
    let entry = graph.entry().index();
    if matches!(infos[entry].format, Format::Json | Format::Opaque) {
        return Err(Diagnostic::new(
            Code::Unsupported,
            &infos[entry].path,
            "the entry is not JavaScript",
        ));
    }
    let linked = link::link(graph, &link_modules, entry, &virtual_ids, options)?;
    drop(link_modules);
    for (index, role) in linked.roles.iter().enumerate() {
        if *role == Role::Json
            && serde_json::from_str::<serde_json::Value>(&sources[index].text).is_err()
        {
            let imported = linked.order.contains(&link::Step::Json(index));
            if imported {
                return Err(Diagnostic::new(
                    Code::Parse,
                    &sources[index].path,
                    "not valid JSON, and an ES module imports it, so Node fails to link the program",
                ));
            }
        }
    }
    let link_time = started.elapsed();

    let started = Instant::now();
    let outdir_parts = components(outdir);
    let root_rel = relative_url(&outdir_parts, &components(&root), true);
    let file_name = format!(
        "{}.mjs",
        Path::new(&sources[entry].path)
            .file_stem()
            .map_or_else(|| "bundle".to_string(), |s| s.to_string_lossy().to_string())
    );
    let mut units: Vec<emit::Unit<'_>> = Vec::with_capacity(sources.len());
    for ((source, info), (parsed, allocator)) in sources
        .iter()
        .zip(&infos)
        .zip(parsed.into_iter().zip(&allocators))
    {
        units.push(emit::Unit {
            rel: relative_url(&outdir_parts, &components(&source.absolute), false),
            source,
            info,
            parsed,
            allocator,
        });
    }
    let defines = Defines::new(options.defines.clone());
    let bundle = emit::emit(
        &mut units, &linked, options, &defines, &root_rel, &file_name,
    )?;
    let print_time = started.elapsed();
    let modules = linked
        .roles
        .iter()
        .filter(|role| !matches!(role, Role::Excluded))
        .count();
    Ok(BuildOutput {
        file_name,
        code: bundle.code,
        map: bundle.map,
        modules,
        timings: Timings {
            parse: parse_time,
            link: link_time,
            print: print_time,
            write: Duration::ZERO,
        },
    })
}

/// Writes the bundle and its map into `outdir`, each through a temp file and
/// a rename, the map first: a build killed partway leaves either the old
/// bundle or the new one in place, never a torn file.
pub fn write(outdir: &Path, output: &mut BuildOutput) -> Result<(), Diagnostic> {
    let started = Instant::now();
    let io = |error: std::io::Error| {
        Diagnostic::new(Code::Io, "", format!("{}: {error}", outdir.display()))
    };
    std::fs::create_dir_all(outdir).map_err(io)?;
    opal_core::atomic::write_atomic(
        &outdir.join(format!("{}.map", output.file_name)),
        output.map.as_bytes(),
        None,
    )
    .map_err(io)?;
    opal_core::atomic::write_atomic(
        &outdir.join(&output.file_name),
        output.code.as_bytes(),
        None,
    )
    .map_err(io)?;
    output.timings.write = started.elapsed();
    Ok(())
}

fn module_id(graph: &ModuleGraph, index: usize) -> ModuleId {
    graph
        .id_of(&graph.modules()[index].path)
        .expect("every module in the graph has an id")
}

/// An absolute path's normal components, `..` applied.
fn components(path: &Path) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(part) => out.push(part.to_string_lossy().to_string()),
            std::path::Component::ParentDir => {
                out.pop();
            }
            _ => {}
        }
    }
    out
}

/// The relative URL from directory `from` to `to`, percent-encoded per
/// segment; with `directory`, ending in `/`.
fn relative_url(from: &[String], to: &[String], directory: bool) -> String {
    let common = from.iter().zip(to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = Vec::new();
    for _ in common..from.len() {
        parts.push("..".to_string());
    }
    for segment in &to[common..] {
        parts.push(encode_segment(segment));
    }
    let mut url = if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    };
    if !url.starts_with('.') {
        url.insert_str(0, "./");
    }
    if directory {
        url.push('/');
    }
    url
}

fn encode_segment(segment: &str) -> String {
    let mut out = String::new();
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b'!'
            | b'$'
            | b'&'
            | b'\''
            | b'('
            | b')'
            | b'*'
            | b'+'
            | b','
            | b';'
            | b'='
            | b':'
            | b'@' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relative_urls_walk_up_and_encode() {
        let out = components(Path::new("/p/dist"));
        assert_eq!(
            relative_url(&out, &components(Path::new("/p/src/a b.js")), false),
            "../src/a%20b.js"
        );
        assert_eq!(
            relative_url(&out, &components(Path::new("/p")), true),
            "../"
        );
        assert_eq!(
            relative_url(&out, &components(Path::new("/p/dist/x.js")), false),
            "./x.js"
        );
    }
}
