//! ES modules on V8: the registry of compiled modules and what each one's
//! imports were linked to, and the hooks V8 calls into (resolving an import
//! while instantiating, `import.meta`, `import()`, a synthetic module's
//! evaluation). Which file a specifier names, what format it is, and what to
//! do with it is decided in JavaScript (js/bootstrap/esm.js), as in Node's
//! ESM loader; this side does what only the engine can.

use std::collections::HashMap;

/// Every module compiled in this process, by id (its index).
#[derive(Default)]
pub struct Registry {
    modules: Vec<Entry>,
    /// Module identity hash to ids, to find a module V8 hands back.
    by_hash: HashMap<i32, Vec<usize>>,
}

struct Entry {
    module: v8::Global<v8::Module>,
    url: String,
    /// What each import request was linked to, in request order:
    /// (specifier, `type` attribute, target id).
    links: Vec<(String, String, usize)>,
    /// For a synthetic module: what sets its exports when it is evaluated.
    evaluator: Option<v8::Global<v8::Function>>,
}

/// The JavaScript side's hooks, from main.js.
pub struct Hooks {
    pub import_meta: v8::Global<v8::Function>,
    pub import_dynamic: v8::Global<v8::Function>,
}

fn registry<'a>(scope: &'a mut v8::PinScope) -> &'a mut Registry {
    if scope.get_slot::<Registry>().is_none() {
        scope.set_slot(Registry::default());
    }
    scope.get_slot_mut::<Registry>().unwrap()
}

fn find(scope: &mut v8::PinScope, module: v8::Local<v8::Module>) -> Option<usize> {
    let hash = module.get_identity_hash().get();
    let candidates = registry(scope)
        .by_hash
        .get(&hash)
        .cloned()
        .unwrap_or_default();
    candidates.into_iter().find(|&id| {
        let global = registry(scope).modules[id].module.clone();
        v8::Local::new(scope, global) == module
    })
}

fn module_of<'s>(scope: &mut v8::PinScope<'s, '_>, id: usize) -> Option<v8::Local<'s, v8::Module>> {
    let global = registry(scope).modules.get(id)?.module.clone();
    Some(v8::Local::new(scope, global))
}

fn add(
    scope: &mut v8::PinScope,
    module: v8::Local<v8::Module>,
    url: String,
    evaluator: Option<v8::Global<v8::Function>>,
) -> usize {
    let hash = module.get_identity_hash().get();
    let global = v8::Global::new(scope, module);
    let registry = registry(scope);
    let id = registry.modules.len();
    registry.modules.push(Entry {
        module: global,
        url,
        links: Vec::new(),
        evaluator,
    });
    registry.by_hash.entry(hash).or_default().push(id);
    id
}

fn type_attribute(
    scope: &mut v8::PinScope,
    attributes: v8::Local<v8::FixedArray>,
    entry_size: usize,
) -> String {
    let mut i = 0;
    while i + 1 < attributes.length() {
        let key = attributes
            .get(scope, i)
            .and_then(|k| v8::Local::<v8::Value>::try_from(k).ok());
        let value = attributes
            .get(scope, i + 1)
            .and_then(|v| v8::Local::<v8::Value>::try_from(v).ok());
        if let (Some(key), Some(value)) = (key, value)
            && key.to_rust_string_lossy(scope) == "type"
        {
            return value.to_rust_string_lossy(scope);
        }
        i += entry_size;
    }
    String::new()
}

fn throw_type_error(scope: &mut v8::PinScope, message: &str) {
    let message = v8::String::new(scope, message).unwrap();
    let error = v8::Exception::type_error(scope, message);
    scope.throw_exception(error);
}

fn id_arg(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Option<usize> {
    let id = value.uint32_value(scope)? as usize;
    (id < registry(scope).modules.len()).then_some(id)
}

/// `esmCompile(source, url)`: `[id, requests]`, each request
/// `[specifier, attributes]`, attributes an array of `[key, value]`.
pub fn compile(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let (Ok(source), Ok(url)) = (
        v8::Local::<v8::String>::try_from(args.get(0)),
        v8::Local::<v8::String>::try_from(args.get(1)),
    ) else {
        return throw_type_error(scope, "esmCompile(source, url)");
    };
    let origin = v8::ScriptOrigin::new(
        scope,
        url.into(),
        0,
        0,
        false,
        -1,
        None,
        false,
        false,
        true,
        None,
    );
    let mut source = v8::script_compiler::Source::new(source, Some(&origin));
    let Some(module) = v8::script_compiler::compile_module(scope, &mut source) else {
        return;
    };
    let url = url.to_rust_string_lossy(scope);
    let id = add(scope, module, url, None);
    let requests = module.get_module_requests();
    let mut list = Vec::with_capacity(requests.length());
    for i in 0..requests.length() {
        let Some(request) = requests
            .get(scope, i)
            .and_then(|r| v8::Local::<v8::ModuleRequest>::try_from(r).ok())
        else {
            continue;
        };
        let specifier: v8::Local<v8::Value> = request.get_specifier().into();
        let attributes = request.get_import_attributes();
        // ModuleRequest attributes come as [key, value, location] triples.
        let mut pairs = Vec::new();
        let mut j = 0;
        while j + 1 < attributes.length() {
            let key = attributes
                .get(scope, j)
                .and_then(|k| v8::Local::<v8::Value>::try_from(k).ok());
            let value = attributes
                .get(scope, j + 1)
                .and_then(|v| v8::Local::<v8::Value>::try_from(v).ok());
            if let (Some(key), Some(value)) = (key, value) {
                pairs.push(v8::Array::new_with_elements(scope, &[key, value]).into());
            }
            j += 3;
        }
        let pairs = v8::Array::new_with_elements(scope, &pairs);
        list.push(v8::Array::new_with_elements(scope, &[specifier, pairs.into()]).into());
    }
    let id = v8::Integer::new(scope, id as i32).into();
    let list = v8::Array::new_with_elements(scope, &list).into();
    rv.set(v8::Array::new_with_elements(scope, &[id, list]).into());
}

fn evaluation_steps<'s>(
    context: v8::Local<'s, v8::Context>,
    module: v8::Local<'s, v8::Module>,
) -> Option<v8::Local<'s, v8::Value>> {
    v8::callback_scope!(unsafe scope, context);
    let id = find(scope, module)?;
    let evaluator = registry(scope).modules[id].evaluator.clone()?;
    let evaluator = v8::Local::new(scope, evaluator);
    let undefined = v8::undefined(scope).into();
    let id_value = v8::Integer::new(scope, id as i32).into();
    evaluator.call(scope, undefined, &[id_value])?;
    // A resolved promise: the evaluation finished.
    let resolver = v8::PromiseResolver::new(scope)?;
    resolver.resolve(scope, undefined);
    Some(v8::Local::new(scope, v8::Global::new(scope, resolver.get_promise(scope))).into())
}

/// `esmSynthetic(url, exportNames, evaluator)`: a module whose exports
/// `evaluator(id)` sets with esmSetExport when the module is evaluated: the
/// facade over a CommonJS module, a JSON file or a built-in.
pub fn synthetic(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let (Ok(url), Ok(names), Ok(evaluator)) = (
        v8::Local::<v8::String>::try_from(args.get(0)),
        v8::Local::<v8::Array>::try_from(args.get(1)),
        v8::Local::<v8::Function>::try_from(args.get(2)),
    ) else {
        return throw_type_error(scope, "esmSynthetic(url, names, evaluator)");
    };
    let mut export_names = Vec::with_capacity(names.length() as usize);
    for i in 0..names.length() {
        if let Some(Ok(name)) = names
            .get_index(scope, i)
            .map(v8::Local::<v8::String>::try_from)
        {
            export_names.push(name);
        }
    }
    let module = v8::Module::create_synthetic_module(scope, url, &export_names, evaluation_steps);
    let evaluator = v8::Global::new(scope, evaluator);
    let url = url.to_rust_string_lossy(scope);
    let id = add(scope, module, url, Some(evaluator));
    rv.set(v8::Integer::new(scope, id as i32).into());
}

/// `esmSetExport(id, name, value)`, from a synthetic module's evaluator.
pub fn set_export<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    _: v8::ReturnValue<'s, v8::Value>,
) {
    let Some(id) = id_arg(scope, args.get(0)) else {
        return throw_type_error(scope, "esmSetExport: no such module");
    };
    let Ok(name) = v8::Local::<v8::String>::try_from(args.get(1)) else {
        return throw_type_error(scope, "esmSetExport: name must be a string");
    };
    let module = module_of(scope, id).unwrap();
    let _ = module.set_synthetic_module_export(scope, name, args.get(2));
}

/// `esmLink(id, [[specifier, type, targetId], ...])`: what each of a
/// module's imports leads to, before it is instantiated.
pub fn link(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    _: v8::ReturnValue<v8::Value>,
) {
    let Some(id) = id_arg(scope, args.get(0)) else {
        return throw_type_error(scope, "esmLink: no such module");
    };
    let Ok(list) = v8::Local::<v8::Array>::try_from(args.get(1)) else {
        return throw_type_error(scope, "esmLink(id, links)");
    };
    let mut links = Vec::with_capacity(list.length() as usize);
    for i in 0..list.length() {
        let Some(Ok(item)) = list
            .get_index(scope, i)
            .map(v8::Local::<v8::Array>::try_from)
        else {
            continue;
        };
        let specifier = item
            .get_index(scope, 0)
            .unwrap()
            .to_rust_string_lossy(scope);
        let kind = item
            .get_index(scope, 1)
            .unwrap()
            .to_rust_string_lossy(scope);
        let target = item.get_index(scope, 2).unwrap();
        let Some(target) = id_arg(scope, target) else {
            return throw_type_error(scope, "esmLink: no such target module");
        };
        links.push((specifier, kind, target));
    }
    registry(scope).modules[id].links = links;
}

fn resolve_callback<'s>(
    context: v8::Local<'s, v8::Context>,
    specifier: v8::Local<'s, v8::String>,
    attributes: v8::Local<'s, v8::FixedArray>,
    referrer: v8::Local<'s, v8::Module>,
) -> Option<v8::Local<'s, v8::Module>> {
    v8::callback_scope!(unsafe scope, context);
    let id = find(scope, referrer)?;
    let specifier = specifier.to_rust_string_lossy(scope);
    let kind = type_attribute(scope, attributes, 3);
    let target = registry(scope).modules[id]
        .links
        .iter()
        .find(|(s, k, _)| *s == specifier && *k == kind)
        .map(|&(_, _, target)| target);
    let Some(target) = target else {
        let url = registry(scope).modules[id].url.clone();
        throw_type_error(
            scope,
            &format!("{url}: import of {specifier:?} was never linked"),
        );
        return None;
    };
    let module = module_of(scope, target)?;
    Some(v8::Local::new(scope, v8::Global::new(scope, module)))
}

/// `esmInstantiate(id)`: links the module's graph (V8 checks every named
/// import against its module's exports here).
pub fn instantiate(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let Some(id) = id_arg(scope, args.get(0)) else {
        return throw_type_error(scope, "esmInstantiate: no such module");
    };
    let module = module_of(scope, id).unwrap();
    if module.get_status() != v8::ModuleStatus::Uninstantiated {
        rv.set_bool(true);
        return;
    }
    if let Some(ok) = module.instantiate_module(scope, resolve_callback) {
        rv.set_bool(ok);
    }
}

/// `esmEvaluate(id)`: runs the graph; a promise, which a top-level await
/// leaves pending.
pub fn evaluate(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let Some(id) = id_arg(scope, args.get(0)) else {
        return throw_type_error(scope, "esmEvaluate: no such module");
    };
    let module = module_of(scope, id).unwrap();
    if let Some(result) = module.evaluate(scope) {
        rv.set(result);
    }
}

/// `esmNamespace(id)`.
pub fn namespace(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let Some(id) = id_arg(scope, args.get(0)) else {
        return throw_type_error(scope, "esmNamespace: no such module");
    };
    let module = module_of(scope, id).unwrap();
    rv.set(module.get_module_namespace());
}

/// `esmStatus(id)`: `[status, isGraphAsync]`, status as V8 numbers it
/// (0 uninstantiated .. 4 evaluated, 5 errored).
pub fn status(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let Some(id) = id_arg(scope, args.get(0)) else {
        return throw_type_error(scope, "esmStatus: no such module");
    };
    let module = module_of(scope, id).unwrap();
    let status = match module.get_status() {
        v8::ModuleStatus::Uninstantiated => 0,
        v8::ModuleStatus::Instantiating => 1,
        v8::ModuleStatus::Instantiated => 2,
        v8::ModuleStatus::Evaluating => 3,
        v8::ModuleStatus::Evaluated => 4,
        v8::ModuleStatus::Errored => 5,
    };
    let is_async =
        module.get_status() != v8::ModuleStatus::Uninstantiated && module.is_graph_async();
    let status = v8::Integer::new(scope, status).into();
    let is_async = v8::Boolean::new(scope, is_async).into();
    rv.set(v8::Array::new_with_elements(scope, &[status, is_async]).into());
}

/// V8's import.meta hook: main.js's initializeImportMeta(meta, url).
pub extern "C" fn import_meta(
    context: v8::Local<v8::Context>,
    module: v8::Local<v8::Module>,
    meta: v8::Local<v8::Object>,
) {
    v8::callback_scope!(unsafe scope, context);
    v8::scope!(let scope, scope);
    let Some(id) = find(scope, module) else {
        return;
    };
    let url = registry(scope).modules[id].url.clone();
    let Some(hook) = scope.get_slot::<Hooks>().map(|h| h.import_meta.clone()) else {
        return;
    };
    let hook = v8::Local::new(scope, hook);
    let url = v8::String::new(scope, &url).unwrap().into();
    let undefined = v8::undefined(scope).into();
    hook.call(scope, undefined, &[meta.into(), url]);
}

/// V8's import() hook: main.js's importDynamic(specifier, referrer,
/// attributes), which returns the promise.
pub fn import_dynamic<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    _: v8::Local<'s, v8::Data>,
    resource_name: v8::Local<'s, v8::Value>,
    specifier: v8::Local<'s, v8::String>,
    attributes: v8::Local<'s, v8::FixedArray>,
) -> Option<v8::Local<'s, v8::Promise>> {
    let kind = type_attribute(scope, attributes, 2);
    let hook = scope.get_slot::<Hooks>()?.import_dynamic.clone();
    let hook = v8::Local::new(scope, hook);
    let kind = v8::String::new(scope, &kind).unwrap().into();
    let undefined = v8::undefined(scope).into();
    let promise = hook.call(scope, undefined, &[specifier.into(), resource_name, kind])?;
    v8::Local::<v8::Promise>::try_from(promise).ok()
}
