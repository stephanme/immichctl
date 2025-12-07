fn main() {
    // Source OpenAPI spec
    let src = "./immich-openapi-specs.json";
    println!("cargo:rerun-if-changed={}", src);

    // Parse OpenAPI v3 spec
    let file = std::fs::File::open(src).expect("failed to open OpenAPI spec file");
    let mut spec: openapiv3::OpenAPI =
        serde_json::from_reader(file).expect("failed to parse OpenAPI spec");

    // Explicit allowlist of path + method pairs.
    // TODO: Adjust this list to the exact endpoints you need.
    use std::collections::HashSet;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum Method {
        Get,
        Post,
        Put,
        Delete,
        Options,
        Head,
        Patch,
        Trace,
    }

    let allowed: HashSet<(String, Method)> = [
        // Example entries (replace with Immich endpoints you require):
        ("/server/version".to_string(), Method::Get),
        ("/auth/validateToken".to_string(), Method::Post),
    ]
    .into_iter()
    .collect();

    // Retain only paths that have at least one allowed operation.
    spec.paths.paths.retain(|path, item| {
        let Some(pi) = item.as_item() else {
            return false;
        };
        let pairs = [
            (pi.get.as_ref(), Method::Get),
            (pi.post.as_ref(), Method::Post),
            (pi.put.as_ref(), Method::Put),
            (pi.delete.as_ref(), Method::Delete),
            (pi.options.as_ref(), Method::Options),
            (pi.head.as_ref(), Method::Head),
            (pi.patch.as_ref(), Method::Patch),
            (pi.trace.as_ref(), Method::Trace),
        ];

        pairs.iter().any(|(op, m)| {
            op.is_some() && allowed.contains(&(path.clone(), *m))
        })
    });

    // Generate Rust client code using progenitor
    let mut generator = progenitor::Generator::default();
    let tokens = generator
        .generate_tokens(&spec)
        .expect("progenitor token generation failed");
    let ast = syn::parse2(tokens).expect("failed to parse generated tokens");
    let content = prettyplease::unparse(&ast);

    let mut out_file =
        std::path::Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR missing")).to_path_buf();
    out_file.push("codegen.rs");
    std::fs::write(&out_file, content).expect("failed to write generated code");

    // Optional: write filtered spec for inspection
    let mut filtered = out_file.clone();
    filtered.set_file_name("filtered-openapi.json");
    std::fs::write(
        &filtered,
        serde_json::to_string_pretty(&spec).expect("failed to serialize filtered spec"),
    )
    .expect("failed to write filtered spec");
}
