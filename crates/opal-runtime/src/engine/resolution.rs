//! The module resolution ops: Node's rules, from
//! opal_core::graph::node_resolution, with one resolver for the process, so
//! that what it learns about the filesystem (stats, real paths, package.json
//! files) is learned once, as in Node.

use std::path::{Path, PathBuf};

use opal_core::graph::node_resolution::{Failure, NodeResolver, Resolved};

/// The process's resolver, in an isolate slot.
pub struct Resolver(pub NodeResolver);

fn resolver<'a>(scope: &'a mut v8::PinScope) -> &'a mut NodeResolver {
    if scope.get_slot::<Resolver>().is_none() {
        let exec = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("/usr/local/bin/opal"));
        scope.set_slot(Resolver(NodeResolver::new(&exec)));
    }
    &mut scope.get_slot_mut::<Resolver>().unwrap().0
}

fn text<'s>(scope: &v8::PinScope<'s, '_>, value: &str) -> v8::Local<'s, v8::Value> {
    v8::String::new(scope, value).unwrap().into()
}

fn optional<'s>(scope: &v8::PinScope<'s, '_>, value: Option<&str>) -> v8::Local<'s, v8::Value> {
    match value {
        Some(value) => text(scope, value),
        None => v8::null(scope).into(),
    }
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// `resolve(specifier, parent, mode)`, mode 0 for require and 1 for import.
/// Returns the path, or one of:
///   ["builtin", name]                 an import of node:name
///   ["url", url]                      an import of another kind of URL
///   ["not-found"]                     require's "Cannot find module"
///   ["bad-main", filename, pjson]     a package.json main that names nothing
///   ["esm-not-found", request, path]  an exports/imports target that isn't there
///   ["node", code, hint, ...args]     a Node error, made by internal/errors
pub fn resolve(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let specifier = args.get(0).to_rust_string_lossy(scope);
    let parent = args.get(1);
    let parent =
        (!parent.is_null_or_undefined()).then(|| PathBuf::from(parent.to_rust_string_lossy(scope)));
    let mode = args.get(2).int32_value(scope).unwrap_or(0);
    let is_main = args.get(3).is_true();
    let result = {
        let resolver = resolver(scope);
        match (mode, &parent) {
            (1, Some(parent)) => resolver.import(&specifier, parent),
            _ => resolver
                .require(&specifier, parent.as_deref(), is_main)
                .map(Resolved::File),
        }
    };
    let value: v8::Local<v8::Value> = match result {
        Ok(Resolved::File(path)) => text(scope, &path_text(&path)),
        Ok(Resolved::Builtin(name)) => {
            let items = [text(scope, "builtin"), text(scope, &name)];
            v8::Array::new_with_elements(scope, &items).into()
        }
        Ok(Resolved::Url(url)) => {
            let items = [text(scope, "url"), text(scope, &url)];
            v8::Array::new_with_elements(scope, &items).into()
        }
        Err(Failure::NotFound) => {
            let items = [text(scope, "not-found")];
            v8::Array::new_with_elements(scope, &items).into()
        }
        Err(Failure::BadMain {
            filename,
            package_json,
        }) => {
            let items = [
                text(scope, "bad-main"),
                text(scope, &path_text(&filename)),
                text(scope, &path_text(&package_json)),
            ];
            v8::Array::new_with_elements(scope, &items).into()
        }
        Err(Failure::EsmNotFound { request, path }) => {
            let path = path.as_deref().map(path_text);
            let items = [
                text(scope, "esm-not-found"),
                text(scope, &request),
                optional(scope, path.as_deref()),
            ];
            v8::Array::new_with_elements(scope, &items).into()
        }
        Err(Failure::Node { code, args, hint }) => {
            let mut items = vec![
                text(scope, "node"),
                text(scope, code),
                optional(scope, hint.as_deref()),
            ];
            for arg in &args {
                items.push(optional(scope, arg.as_deref()));
            }
            v8::Array::new_with_elements(scope, &items).into()
        }
    };
    rv.set(value);
}

/// `takeResolutionWarnings()`.
pub fn take_warnings(
    scope: &mut v8::PinScope,
    _: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let warnings = std::mem::take(&mut resolver(scope).warnings);
    let items: Vec<v8::Local<v8::Value>> = warnings
        .iter()
        .map(|w| {
            let parts = [
                text(scope, &w.message),
                text(scope, w.kind),
                text(scope, w.code),
            ];
            v8::Array::new_with_elements(scope, &parts).into()
        })
        .collect();
    rv.set(v8::Array::new_with_elements(scope, &items).into());
}

/// `packageType(path)`.
pub fn package_type(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let path = PathBuf::from(args.get(0).to_rust_string_lossy(scope));
    let kind = match resolver(scope).package_scope(&path) {
        Ok(Some(scope)) => scope
            .module_type
            .clone()
            .unwrap_or_else(|| "none".to_string()),
        _ => "none".to_string(),
    };
    rv.set(text(scope, &kind));
}
