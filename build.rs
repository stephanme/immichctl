use std::collections::HashMap;

fn main() {
    // Source OpenAPI spec
    let src = "./immich-openapi-specs.json";
    println!("cargo:rerun-if-changed={}", src);

    // Parse OpenAPI v3 spec
    let file = std::fs::File::open(src).expect("failed to open OpenAPI spec file");
    let mut spec: openapiv3::OpenAPI =
        serde_json::from_reader(file).expect("failed to parse OpenAPI spec");

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum Method {
        Get,
        Post,
        // Put,
        // Delete,
        // Options,
        // Head,
        // Patch,
        // Trace,
    }

    // Immich endpoints required by immichctl
    let allowed: HashMap<&str, Vec<Method>> = HashMap::from([
        ("/server/version", vec![Method::Get]),
        ("/auth/validateToken", vec![Method::Post]),
        ("/search/metadata", vec![Method::Post]),
        ("/tags", vec![Method::Get]),
    ]);

    // Retain only paths that have at least one allowed operation.
    spec.paths.paths.retain(|path, item| {
        let Some(_pi) = item.as_item() else {
            return false;
        };
        allowed.contains_key(path.as_str())
    });

    // Generate Rust client code using progenitor
    let settings = progenitor::GenerationSettings::default();
    let mut generator = progenitor::Generator::new(&settings);
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
