//! What a build is asked to do.

use opal_core::graph::ResolverOptions;

/// The runtime the bundle is for. `SEMANTICS.md` ("The reference").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    Node,
    Browser,
}

impl Target {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "node" => Some(Target::Node),
            "browser" => Some(Target::Browser),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Target::Node => "node",
            Target::Browser => "browser",
        }
    }

    /// The resolver options that make opal-core resolve as this target's
    /// runtime does. `import`/`require` and `default` are added per edge by
    /// the resolver itself.
    pub fn resolver_options(self, jsx_runtime: Option<String>) -> ResolverOptions {
        let conditions = match self {
            // lib/internal/modules/esm/utils.js:74-87 at v24.19.0.
            Target::Node => vec!["node", "module-sync", "node-addons"],
            Target::Browser => vec!["browser"],
        };
        ResolverOptions {
            conditions: conditions.into_iter().map(str::to_string).collect(),
            jsx_runtime,
            // A JavaScript module's imports resolve as Node resolves them,
            // or the bundle would run programs Node refuses to.
            node_exact: true,
            ..ResolverOptions::default()
        }
    }
}

/// `--define <path>=<expression>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Define {
    /// `process.env.NODE_ENV`, as segments.
    pub path: Vec<String>,
    /// JavaScript source of the replacement expression.
    pub value: String,
}

impl Define {
    /// Parses `a.b.c=value`. The value is JavaScript, so a string needs its
    /// quotes: `process.env.NODE_ENV='"production"'`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let (path, value) = text
            .split_once('=')
            .ok_or_else(|| format!("--define {text:?} has no '='"))?;
        let segments: Vec<String> = path.split('.').map(str::to_string).collect();
        let valid = segments.iter().all(|segment| {
            let mut chars = segment.chars();
            chars
                .next()
                .is_some_and(|c| c == '_' || c == '$' || c.is_ascii_alphabetic())
                && chars.all(|c| c == '_' || c == '$' || c.is_ascii_alphanumeric())
        });
        if !valid {
            return Err(format!(
                "--define {path:?} is not a dotted path of identifiers"
            ));
        }
        if value.trim().is_empty() {
            return Err(format!("--define {path:?} has an empty value"));
        }
        Ok(Self {
            path: segments,
            value: value.to_string(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct BuildOptions {
    pub target: Target,
    /// Package names (`react` also covers `react/jsx-runtime`) or
    /// root-relative paths kept out of the bundle.
    pub externals: Vec<String>,
    pub defines: Vec<Define>,
    /// The JSX automatic runtime's import source, when a tsconfig does not
    /// name one.
    pub jsx_import_source: String,
    /// Rule 17: import non-JavaScript files with Vite's semantics.
    pub assets: bool,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            target: Target::Node,
            externals: Vec::new(),
            defines: Vec::new(),
            jsx_import_source: "react".to_string(),
            assets: false,
        }
    }
}

impl BuildOptions {
    /// Whether a specifier names an external.
    pub fn is_external(&self, specifier: &str) -> bool {
        self.externals.iter().any(|external| {
            specifier == external
                || specifier
                    .strip_prefix(external.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
        })
    }
}
