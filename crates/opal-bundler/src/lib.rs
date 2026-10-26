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
use std::sync::{OnceLock, mpsc};
use std::time::{Duration, Instant};

/// The stack each build thread gets. Parsing, analysis, and printing
/// recurse once per level of nesting, and a module may nest up to
/// [`load::MAX_DEPTH`] levels; the main thread's 8 MiB is not enough for
/// that, let alone a spawned thread's default 2 MiB. Only the pages used
/// are committed.
pub const STACK_SIZE: usize = 256 * 1024 * 1024;

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
    /// Files written next to the bundle (rule 17): path relative to the
    /// out-dir, and bytes.
    pub assets: Vec<(String, Vec<u8>)>,
    /// Modules in the bundle.
    pub modules: usize,
    /// Statements left out only because a package's `sideEffects` said they
    /// do nothing (rule 15): module path, and how many.
    pub trusted: Vec<(String, usize)>,
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

    // Per module, before any thread starts: compiler settings, the
    // package's sideEffects word, and where the file is from the out-dir.
    // A settings error is the module's to report, in module order with the
    // parse errors, as a sequential build would.
    let settings: Vec<Option<Result<tsconfig::TsSettings, Diagnostic>>> = sources
        .iter()
        .map(|source| {
            (!matches!(source.format, Format::Json | Format::Opaque))
                .then(|| load::settings_for(&mut tsconfigs, source, &options.jsx_import_source))
        })
        .collect();
    let side_effect_free: Vec<bool> = sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            source.format == Format::Esm
                && index < graph.len()
                && packages.side_effect_free(&source.absolute)
        })
        .collect();
    let outdir_parts = components(outdir);
    let root_rel = relative_url(&outdir_parts, &components(&root), true);
    let rels: Vec<String> = sources
        .iter()
        .map(|source| relative_url(&outdir_parts, &components(&source.absolute), false))
        .collect();
    let defines = Defines::new(options.defines.clone());
    let entry = graph.entry().index();
    let file_name = format!(
        "{}.mjs",
        Path::new(&sources[entry].path)
            .file_stem()
            .map_or_else(|| "bundle".to_string(), |s| s.to_string_lossy().to_string())
    );

    // Parse and print run on every core. A module's AST lives in an arena
    // its thread owns, so each thread parses its share, hands linking the
    // facts it found, waits, and then prints the same share; only the
    // facts and the printed text cross threads.
    let threads = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(sources.len())
        .max(1);
    let mut by_size: Vec<usize> = (0..sources.len()).collect();
    by_size.sort_by_key(|index| std::cmp::Reverse(sources[*index].text.len()));
    let mut shares: Vec<Vec<usize>> = vec![Vec::new(); threads];
    for (position, index) in by_size.into_iter().enumerate() {
        shares[position % threads].push(index);
    }

    let infos_cell: OnceLock<Vec<Info>> = OnceLock::new();
    let linked_cell: OnceLock<link::Linked> = OnceLock::new();
    let shared_cell: OnceLock<emit::Shared> = OnceLock::new();
    let mut parse_time = Duration::ZERO;
    let mut link_time = Duration::ZERO;
    let mut print_started = Instant::now();
    type Facts = (Info, Option<(Vec<String>, Vec<String>)>);
    let printed: Result<(HashMap<usize, emit::Piece>, emit::Uses), Diagnostic> = std::thread::scope(
        |scope| {
            let (facts_tx, facts_rx) = mpsc::channel::<(usize, Result<Facts, Diagnostic>)>();
            let (piece_tx, piece_rx) =
                mpsc::channel::<(usize, Result<Option<(emit::Piece, emit::Uses)>, Diagnostic>)>();
            let stop = |go: &[mpsc::Sender<bool>]| {
                for sender in go {
                    let _ = sender.send(false);
                }
            };
            let mut go = Vec::with_capacity(threads);
            for share in shares {
                let (go_tx, go_rx) = mpsc::channel::<bool>();
                go.push(go_tx);
                let facts_tx = facts_tx.clone();
                let piece_tx = piece_tx.clone();
                let (sources, settings, rels, defines) = (&sources, &settings, &rels, &defines);
                let (infos_cell, linked_cell, shared_cell) =
                    (&infos_cell, &linked_cell, &shared_cell);
                let spawned = std::thread::Builder::new()
                    .name("opal-build".to_string())
                    .stack_size(STACK_SIZE)
                    .spawn_scoped(scope, move || {
                        let allocators: Vec<Allocator> =
                            share.iter().map(|_| Allocator::default()).collect();
                        let mut parsed: Vec<Option<Parsed<'_>>> = Vec::with_capacity(share.len());
                        for (slot, &index) in share.iter().enumerate() {
                            let source = &sources[index];
                            let result = match &settings[index] {
                                None => Ok((Info::opaque(source), None, None)),
                                Some(Err(diagnostic)) => Err(diagnostic.clone()),
                                Some(Ok(settings)) => {
                                    load::parse(&allocators[slot], source, settings).and_then(
                                        |module| {
                                            let info = analyze::analyze(source, &module)?;
                                            let lexed = (source.format == Format::Cjs)
                                                .then(|| cjs_lexer::exports(&source.text));
                                            Ok((info, lexed, Some(module)))
                                        },
                                    )
                                }
                            };
                            match result {
                                Ok((info, lexed, module)) => {
                                    parsed.push(module);
                                    let _ = facts_tx.send((index, Ok((info, lexed))));
                                }
                                Err(diagnostic) => {
                                    parsed.push(None);
                                    let _ = facts_tx.send((index, Err(diagnostic)));
                                }
                            }
                        }
                        drop(facts_tx);
                        if !go_rx.recv().unwrap_or(false) {
                            return;
                        }
                        let (Some(infos), Some(linked), Some(shared)) =
                            (infos_cell.get(), linked_cell.get(), shared_cell.get())
                        else {
                            return;
                        };
                        for (slot, &index) in share.iter().enumerate() {
                            let Some(module) = parsed[slot].take() else {
                                continue;
                            };
                            let mut unit = emit::Unit {
                                source: &sources[index],
                                info: &infos[index],
                                parsed: Some(module),
                                allocator: &allocators[slot],
                                rel: rels[index].clone(),
                            };
                            let _ = piece_tx.send((
                                index,
                                emit::print(index, &mut unit, linked, options, defines, shared),
                            ));
                        }
                    });
                if let Err(error) = spawned {
                    stop(&go);
                    return Err(Diagnostic::new(
                        Code::Io,
                        "",
                        format!("could not start a build thread: {error}"),
                    ));
                }
            }
            drop(facts_tx);
            drop(piece_tx);

            // Facts from every module, and the first error by module order, so
            // a failing build fails the same way however the threads ran.
            let mut infos: Vec<Option<Info>> = (0..sources.len()).map(|_| None).collect();
            let mut lexed: Vec<Option<(Vec<String>, Vec<String>)>> = vec![None; sources.len()];
            let mut first_error: Option<(usize, Diagnostic)> = None;
            for (index, result) in facts_rx {
                match result {
                    Ok((info, found)) => {
                        infos[index] = Some(info);
                        lexed[index] = found;
                    }
                    Err(diagnostic) => {
                        if first_error.as_ref().is_none_or(|(at, _)| index < *at) {
                            first_error = Some((index, diagnostic));
                        }
                    }
                }
            }
            if let Some((_, diagnostic)) = first_error {
                stop(&go);
                return Err(diagnostic);
            }
            let infos: Vec<Info> = infos
                .into_iter()
                .map(|info| info.expect("every module reported"))
                .collect();
            parse_time = started.elapsed();

            let linking = Instant::now();
            if matches!(infos[entry].format, Format::Json | Format::Opaque) {
                stop(&go);
                return Err(Diagnostic::new(
                    Code::Unsupported,
                    &infos[entry].path,
                    "the entry is not JavaScript",
                ));
            }
            let link_modules: Vec<LinkModule<'_>> = (0..sources.len())
                .map(|index| LinkModule {
                    info: &infos[index],
                    text: &sources[index].text,
                    graph_id: (index < graph.len()).then(|| module_id(graph, index)),
                    cjs_lexed: lexed[index].clone(),
                    side_effect_free: side_effect_free[index],
                })
                .collect();
            let linked = match link::link(graph, &link_modules, entry, &virtual_ids, options) {
                Ok(linked) => linked,
                Err(diagnostic) => {
                    stop(&go);
                    return Err(diagnostic);
                }
            };
            drop(link_modules);
            for (index, role) in linked.roles.iter().enumerate() {
                if *role == Role::Json
                    && linked.order.contains(&link::Step::Json(index))
                    && serde_json::from_str::<serde_json::Value>(&sources[index].text).is_err()
                {
                    stop(&go);
                    return Err(Diagnostic::new(
                        Code::Parse,
                        &sources[index].path,
                        "not valid JSON, and an ES module imports it, so Node fails to link the program",
                    ));
                }
            }
            if options.target == Target::Browser
                && let Err(diagnostic) = check_browser_fields(&linked, &sources)
            {
                stop(&go);
                return Err(diagnostic);
            }
            let info_refs: Vec<&Info> = infos.iter().collect();
            let shared = emit::Shared::prepare(&linked, &info_refs);
            drop(info_refs);
            let _ = infos_cell.set(infos);
            let _ = linked_cell.set(linked);
            let _ = shared_cell.set(shared);
            link_time = linking.elapsed();
            print_started = Instant::now();
            for sender in &go {
                let _ = sender.send(true);
            }

            let mut pieces = HashMap::new();
            let mut uses = emit::Uses::default();
            let mut first_error: Option<(usize, Diagnostic)> = None;
            for (index, result) in piece_rx {
                match result {
                    Ok(Some((piece, used))) => {
                        uses.merge(used);
                        pieces.insert(index, piece);
                    }
                    Ok(None) => {}
                    Err(diagnostic) => {
                        if first_error.as_ref().is_none_or(|(at, _)| index < *at) {
                            first_error = Some((index, diagnostic));
                        }
                    }
                }
            }
            match first_error {
                Some((_, diagnostic)) => Err(diagnostic),
                None => Ok((pieces, uses)),
            }
        },
    );
    let (pieces, uses) = printed?;
    let infos = infos_cell.into_inner().expect("linking ran");
    let linked = linked_cell.into_inner().expect("linking ran");
    let shared = shared_cell.into_inner().expect("linking ran");

    let metas: Vec<emit::Meta<'_>> = sources
        .iter()
        .zip(&infos)
        .zip(rels)
        .map(|((source, info), rel)| emit::Meta { source, info, rel })
        .collect();
    let bundle = emit::assemble(
        pieces, uses, &metas, &linked, options, &shared, &root_rel, &file_name,
    )?;
    let print_time = print_started.elapsed();
    let modules = linked
        .roles
        .iter()
        .filter(|role| !matches!(role, Role::Excluded))
        .count();
    Ok(BuildOutput {
        file_name,
        code: bundle.code,
        map: bundle.map,
        assets: bundle.assets,
        modules,
        trusted: linked
            .trusted
            .iter()
            .map(|(module, count)| (sources[*module].path.clone(), *count))
            .collect(),
        timings: Timings {
            parse: parse_time,
            link: link_time,
            print: print_time,
            write: Duration::ZERO,
        },
    })
}

/// `E-BROWSER-FIELD`: under `--target browser`, a package whose
/// `package.json` `browser` field replaces a file the bundle holds. opal-core
/// does not apply the field, so a browser bundler would have picked another
/// file. A field that replaces only files the bundle does not hold changes
/// nothing (react-dom's maps its server entry points).
fn check_browser_fields(linked: &link::Linked, sources: &[Source]) -> Result<(), Diagnostic> {
    let mut checked: HashMap<std::path::PathBuf, Option<serde_json::Value>> = HashMap::new();
    for (index, role) in linked.roles.iter().enumerate() {
        if matches!(role, Role::Excluded) || index >= linked.graph_modules {
            continue;
        }
        let source = &sources[index];
        let parts: Vec<&str> = source.path.split('/').collect();
        let Some(at) = parts.iter().rposition(|part| *part == "node_modules") else {
            continue;
        };
        let depth = if parts.get(at + 1).is_some_and(|p| p.starts_with('@')) {
            3
        } else {
            2
        };
        if parts.len() <= at + depth {
            continue;
        }
        let package: std::path::PathBuf = source
            .absolute
            .ancestors()
            .nth(parts.len() - (at + depth))
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let browser = checked
            .entry(package.clone())
            .or_insert_with(|| {
                std::fs::read_to_string(package.join("package.json"))
                    .ok()
                    .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                    .and_then(|manifest| {
                        let browser = manifest.get("browser")?.clone();
                        Some(serde_json::json!({ "browser": browser, "main": manifest.get("main").cloned() }))
                    })
            })
            .clone();
        let Some(fields) = browser else {
            continue;
        };
        let within = parts[at + depth..].join("/");
        let normal = |text: &str| text.trim_start_matches("./").to_string();
        let replaced = match &fields["browser"] {
            serde_json::Value::String(_) => fields["main"]
                .as_str()
                .map(normal)
                .is_some_and(|main| within == main || within == format!("{main}.js")),
            serde_json::Value::Object(map) => map.iter().any(|(key, value)| {
                let key = normal(key);
                (within == key || within == format!("{key}.js"))
                    && value.as_str().map(normal) != Some(within.clone())
            }),
            _ => false,
        };
        if replaced {
            return Err(Diagnostic::new(
                Code::BrowserField,
                &source.path,
                "this package's \"browser\" field replaces this file for browsers, and the module graph does not apply that field",
            ));
        }
    }
    Ok(())
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
    for (name, bytes) in &output.assets {
        let path = outdir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        opal_core::atomic::write_atomic(&path, bytes, None).map_err(io)?;
    }
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
