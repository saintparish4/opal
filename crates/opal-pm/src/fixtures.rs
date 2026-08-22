//! A registry made of files, for tests.
//!
//! Feature-gated (`fixtures`) so none of it compiles into a release binary. It
//! exists because the install pipeline's most important properties — crash
//! convergence, concurrency, layout — cannot be tested against the public
//! registry: the network makes them slow and flaky, and a chaos test that
//! downloads express fifty times is a test nobody runs. Pointing
//! `$OPAL_REGISTRY` at one of these exercises the real client, the real
//! integrity check, and the real tarball reader.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::integrity::{Algorithm, Integrity};

/// One package version to publish.
#[derive(Clone, Debug)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub dependencies: BTreeMap<String, String>,
    pub optional_dependencies: BTreeMap<String, String>,
    pub peer_dependencies: BTreeMap<String, String>,
    pub optional_peers: Vec<String>,
    pub bin: BTreeMap<String, String>,
    /// Extra files beyond the generated `package.json`, as (path, contents,
    /// executable).
    pub files: Vec<(String, Vec<u8>, bool)>,
}

impl Package {
    pub fn new(name: &str, version: &str) -> Self {
        Self {
            name: name.to_string(),
            version: version.to_string(),
            dependencies: BTreeMap::new(),
            optional_dependencies: BTreeMap::new(),
            peer_dependencies: BTreeMap::new(),
            optional_peers: Vec::new(),
            bin: BTreeMap::new(),
            files: Vec::new(),
        }
    }

    pub fn dependency(mut self, name: &str, spec: &str) -> Self {
        self.dependencies.insert(name.to_string(), spec.to_string());
        self
    }

    pub fn optional_dependency(mut self, name: &str, spec: &str) -> Self {
        self.optional_dependencies
            .insert(name.to_string(), spec.to_string());
        self
    }

    pub fn optional_peer(mut self, name: &str, spec: &str) -> Self {
        self.peer_dependencies
            .insert(name.to_string(), spec.to_string());
        self.optional_peers.push(name.to_string());
        self
    }

    pub fn file(mut self, path: &str, contents: &str) -> Self {
        self.files
            .push((path.to_string(), contents.as_bytes().to_vec(), false));
        self
    }

    pub fn executable(mut self, path: &str, contents: &str) -> Self {
        self.files
            .push((path.to_string(), contents.as_bytes().to_vec(), true));
        self
    }

    pub fn bin(mut self, command: &str, path: &str) -> Self {
        self.bin.insert(command.to_string(), path.to_string());
        self
    }

    fn manifest_json(&self) -> serde_json::Value {
        let mut value = serde_json::json!({
            "name": self.name,
            "version": self.version,
            "main": "index.js",
        });
        let object = value.as_object_mut().expect("object");
        if !self.dependencies.is_empty() {
            object.insert("dependencies".into(), serde_json::json!(self.dependencies));
        }
        if !self.optional_dependencies.is_empty() {
            object.insert(
                "optionalDependencies".into(),
                serde_json::json!(self.optional_dependencies),
            );
        }
        if !self.peer_dependencies.is_empty() {
            object.insert(
                "peerDependencies".into(),
                serde_json::json!(self.peer_dependencies),
            );
            let meta: BTreeMap<&String, serde_json::Value> = self
                .optional_peers
                .iter()
                .map(|name| (name, serde_json::json!({ "optional": true })))
                .collect();
            object.insert("peerDependenciesMeta".into(), serde_json::json!(meta));
        }
        if !self.bin.is_empty() {
            object.insert("bin".into(), serde_json::json!(self.bin));
        }
        value
    }

    /// The tarball as npm would ship it, `package/` prefix and all.
    fn tarball(&self) -> Vec<u8> {
        let mut files: Vec<(String, Vec<u8>, bool)> = vec![(
            "package.json".to_string(),
            serde_json::to_vec_pretty(&self.manifest_json()).expect("serializable"),
            false,
        )];
        if !self.files.iter().any(|(path, _, _)| path == "index.js") {
            files.push((
                "index.js".to_string(),
                format!("module.exports = {:?};\n", self.name).into_bytes(),
                false,
            ));
        }
        files.extend(self.files.iter().cloned());
        files.sort();

        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        {
            let mut builder = tar::Builder::new(&mut encoder);
            for (path, contents, executable) in files {
                let mut header = tar::Header::new_gnu();
                header.set_size(contents.len() as u64);
                header.set_mode(if executable { 0o755 } else { 0o644 });
                header.set_mtime(0);
                header.set_cksum();
                builder
                    .append_data(&mut header, format!("package/{path}"), contents.as_slice())
                    .expect("append");
            }
            builder.finish().expect("finish");
        }
        encoder.finish().expect("gzip")
    }
}

pub struct FixtureRegistry {
    directory: PathBuf,
    published: BTreeMap<String, Vec<(Package, Integrity, PathBuf)>>,
}

impl FixtureRegistry {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        let directory = directory.into();
        std::fs::create_dir_all(directory.join("tarballs")).expect("create fixture registry");
        Self {
            directory,
            published: BTreeMap::new(),
        }
    }

    /// The value to put in `$OPAL_REGISTRY`.
    pub fn url(&self) -> String {
        format!("file://{}", self.directory.display())
    }

    pub fn publish(&mut self, package: Package) -> &mut Self {
        let tarball = package.tarball();
        let integrity = Integrity::of(Algorithm::Sha512, &tarball);
        let file_name = format!("{}-{}.tgz", package.name.replace('/', "_"), package.version);
        let path = self.directory.join("tarballs").join(file_name);
        std::fs::write(&path, &tarball).expect("write tarball");

        self.published
            .entry(package.name.clone())
            .or_default()
            .push((package.clone(), integrity, path));
        self.write_packument(&package.name);
        self
    }

    fn write_packument(&self, name: &str) {
        let entries = self.published.get(name).expect("published");
        let mut versions = serde_json::Map::new();
        let mut latest = String::new();

        for (package, integrity, path) in entries {
            let mut value = package.manifest_json();
            value.as_object_mut().expect("object").insert(
                "dist".into(),
                serde_json::json!({
                    "tarball": format!("file://{}", path.display()),
                    "integrity": integrity.to_string(),
                }),
            );
            versions.insert(package.version.clone(), value);
            // Fixtures publish in ascending order, so the last one is latest.
            latest = package.version.clone();
        }

        let packument = serde_json::json!({
            "name": name,
            "dist-tags": { "latest": latest },
            "versions": versions,
        });
        let path = self.directory.join(format!("{name}.json"));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create scope directory");
        }
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&packument).expect("serializable"),
        )
        .expect("write packument");
    }
}

/// Writes a project `package.json`.
pub fn write_project(root: &Path, manifest: serde_json::Value) {
    std::fs::create_dir_all(root).expect("create project");
    std::fs::write(
        root.join("package.json"),
        serde_json::to_vec_pretty(&manifest).expect("serializable"),
    )
    .expect("write package.json");
}
