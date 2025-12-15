use openapiv3::ReferenceOr;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Method {
    Get,
    Post,
    Put,
    Delete,
    // Options,
    // Head,
    // Patch,
    // Trace,
}

fn main() {
    // Source OpenAPI spec
    let src = "./immich-openapi-specs.json";
    println!("cargo:rerun-if-changed={}", src);

    // Parse OpenAPI v3 spec
    let file = std::fs::File::open(src).expect("failed to open OpenAPI spec file");
    let mut spec: openapiv3::OpenAPI =
        serde_json::from_reader(file).expect("failed to parse OpenAPI spec");

    // Immich endpoints required by immichctl
    let allowed: HashMap<&str, Vec<Method>> = HashMap::from([
        ("/server/version", vec![Method::Get]),
        ("/auth/validateToken", vec![Method::Post]),
        ("/search/metadata", vec![Method::Post]),
        ("/tags", vec![Method::Get]),
        ("/tags/assets", vec![Method::Put]),
        ("/tags/{id}/assets", vec![Method::Delete]),
        ("/albums", vec![Method::Get]),
    ]);

    // Retain only paths that have at least one allowed operation.
    spec.paths.paths.retain(|path, item| {
        let ReferenceOr::Item(pi) = item else {
            return false;
        };
        let Some(methods) = allowed.get(path.as_str()) else {
            return false;
        };
        // keep only allowed methods for each path
        if pi.get.is_some() && !methods.contains(&Method::Get) {
            pi.get = None;
        }
        if pi.post.is_some() && !methods.contains(&Method::Post) {
            pi.post = None;
        }
        if pi.put.is_some() && !methods.contains(&Method::Put) {
            pi.put = None;
        }
        if pi.delete.is_some() && !methods.contains(&Method::Delete) {
            pi.delete = None;
        }
        pi.head = None;
        pi.options = None;
        pi.patch = None;
        pi.trace = None;
        true
    });

    prune_components_recursive(&mut spec);

    // Generate Rust client code using progenitor
    let mut settings = progenitor::GenerationSettings::default();
    settings.with_derive("PartialEq");
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

/// Recursively mark-and-sweep all OpenAPI components (schemas, parameters, requestBodies, responses, headers)
/// reachable from the given operations.
fn prune_components_recursive(spec: &mut openapiv3::OpenAPI) {
    use openapiv3::*;
    use std::collections::{HashSet, VecDeque};

    let mut used_schemas = HashSet::new();
    let mut used_parameters = HashSet::new();
    let mut used_request_bodies = HashSet::new();
    let mut used_responses = HashSet::new();
    let mut used_headers = HashSet::new();

    let mut queue = VecDeque::new();
    // Add all operation roots (parameters, requestBodies, responses) from allowed paths
    for (_path, item) in &spec.paths.paths {
        let ReferenceOr::Item(pi) = item else {
            continue;
        };
        // dbg!(&pi);
        for op in [
            pi.get.as_ref(),
            pi.post.as_ref(),
            pi.put.as_ref(),
            pi.delete.as_ref(),
            pi.patch.as_ref(),
            pi.options.as_ref(),
            pi.head.as_ref(),
            pi.trace.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            // dbg!(&op);
            for param in &op.parameters {
                visit_parameter_ref(param, &mut queue);
            }
            if let Some(request_body) = &op.request_body {
                visit_request_body_ref(request_body, &mut queue);
            }
            for (_status, resp) in &op.responses.responses {
                visit_response_ref(resp, &mut queue);
            }
        }
    }

    // dbg!(&queue);

    let Some(components) = &mut spec.components else {
        return;
    };
    while let Some(cref) = queue.pop_front() {
        match cref {
            ComponentRef::Schema(name) => {
                if !used_schemas.insert(name.clone()) {
                    continue;
                }
                if let Some(schema) = components.schemas.get(&name) {
                    visit_schema_ref(schema, &mut queue);
                }
            }
            ComponentRef::Parameter(name) => {
                if !used_parameters.insert(name.clone()) {
                    continue;
                }
                if let Some(param) = components.parameters.get(&name) {
                    visit_parameter_ref(param, &mut queue);
                }
            }
            ComponentRef::RequestBody(name) => {
                if !used_request_bodies.insert(name.clone()) {
                    continue;
                }
                if let Some(rb) = components.request_bodies.get(&name) {
                    visit_request_body_ref(rb, &mut queue);
                }
            }
            ComponentRef::Response(name) => {
                if !used_responses.insert(name.clone()) {
                    continue;
                }
                if let Some(resp) = components.responses.get(&name) {
                    visit_response_ref(resp, &mut queue);
                }
            }
            ComponentRef::Header(name) => {
                if !used_headers.insert(name.clone()) {
                    continue;
                }
                if let Some(header) = components.headers.get(&name) {
                    visit_header_ref(header, &mut queue);
                }
            }
        }
    }

    components.schemas.retain(|k, _| used_schemas.contains(k));
    components
        .parameters
        .retain(|k, _| used_parameters.contains(k));
    components
        .request_bodies
        .retain(|k, _| used_request_bodies.contains(k));
    components
        .responses
        .retain(|k, _| used_responses.contains(k));
    components.headers.retain(|k, _| used_headers.contains(k));

    // --- helpers ---
    #[derive(Debug, Clone)]
    enum ComponentRef {
        Schema(String),
        Parameter(String),
        RequestBody(String),
        Response(String),
        Header(String),
    }
    fn visit_schema_ref(r: &ReferenceOr<Schema>, queue: &mut VecDeque<ComponentRef>) {
        match r {
            ReferenceOr::Reference { reference } => {
                if let Some(name) = reference.strip_prefix("#/components/schemas/") {
                    queue.push_back(ComponentRef::Schema(name.to_string()));
                }
            }
            ReferenceOr::Item(schema) => visit_schema(schema, queue),
        }
    }
    fn visit_schema(schema: &Schema, queue: &mut VecDeque<ComponentRef>) {
        use openapiv3::SchemaKind::*;
        match &schema.schema_kind {
            Type(t) => match t {
                openapiv3::Type::Object(o) => {
                    for (_k, v) in &o.properties {
                        match v {
                            ReferenceOr::Reference { .. } => {
                                // Convert &ReferenceOr<Box<Schema>> to ReferenceOr<Schema>
                                if let ReferenceOr::Reference { reference } = v {
                                    visit_schema_ref(
                                        &ReferenceOr::Reference {
                                            reference: reference.clone(),
                                        },
                                        queue,
                                    );
                                }
                            }
                            ReferenceOr::Item(boxed_schema) => {
                                visit_schema(boxed_schema, queue);
                            }
                        }
                    }
                    if let Some(addl) = &o.additional_properties {
                        match addl {
                            openapiv3::AdditionalProperties::Any(_) => {}
                            openapiv3::AdditionalProperties::Schema(r) => {
                                visit_schema_ref(r, queue)
                            }
                        }
                    }
                }
                openapiv3::Type::Array(a) => {
                    if let Some(item_ref) = &a.items {
                        match item_ref {
                            ReferenceOr::Reference { reference } => {
                                if let Some(name) = reference.strip_prefix("#/components/schemas/")
                                {
                                    queue.push_back(ComponentRef::Schema(name.to_string()));
                                }
                            }
                            ReferenceOr::Item(boxed_schema) => {
                                visit_schema(boxed_schema, queue);
                            }
                        }
                    }
                }
                _ => {}
            },
            OneOf { one_of } | AnyOf { any_of: one_of } => {
                for s in one_of {
                    visit_schema_ref(s, queue);
                }
            }
            AllOf { all_of } => {
                for s in all_of {
                    visit_schema_ref(s, queue);
                }
            }
            Not { not } => visit_schema_ref(not, queue),
            _ => {}
        }
        // schema_data?
    }
    fn visit_parameter_ref(r: &ReferenceOr<Parameter>, queue: &mut VecDeque<ComponentRef>) {
        match r {
            ReferenceOr::Reference { reference } => {
                if let Some(name) = reference.strip_prefix("#/components/parameters/") {
                    queue.push_back(ComponentRef::Parameter(name.to_string()));
                }
            }
            ReferenceOr::Item(param) => match &param.parameter_data_ref().format {
                ParameterSchemaOrContent::Schema(reference_or) => {
                    visit_schema_ref(reference_or, queue);
                }
                ParameterSchemaOrContent::Content(index_map) => {
                    for mt in index_map.values() {
                        visit_media_type(mt, queue);
                    }
                }
            },
        }
    }
    fn visit_request_body_ref(r: &ReferenceOr<RequestBody>, queue: &mut VecDeque<ComponentRef>) {
        match r {
            ReferenceOr::Reference { reference } => {
                if let Some(name) = reference.strip_prefix("#/components/requestBodies/") {
                    queue.push_back(ComponentRef::RequestBody(name.to_string()));
                }
            }
            ReferenceOr::Item(rb) => {
                for mt in rb.content.values() {
                    visit_media_type(mt, queue);
                }
            }
        }
    }
    fn visit_response_ref(r: &ReferenceOr<Response>, queue: &mut VecDeque<ComponentRef>) {
        match r {
            ReferenceOr::Reference { reference } => {
                if let Some(name) = reference.strip_prefix("#/components/responses/") {
                    queue.push_back(ComponentRef::Response(name.to_string()));
                }
            }
            ReferenceOr::Item(resp) => {
                for mt in resp.content.values() {
                    visit_media_type(mt, queue);
                }
                for (_k, h) in &resp.headers {
                    visit_header_ref(h, queue);
                }
            }
        }
    }
    fn visit_header_ref(r: &ReferenceOr<Header>, queue: &mut VecDeque<ComponentRef>) {
        match r {
            ReferenceOr::Reference { reference } => {
                if let Some(name) = reference.strip_prefix("#/components/headers/") {
                    queue.push_back(ComponentRef::Header(name.to_string()));
                }
            }
            ReferenceOr::Item(header) => match &header.format {
                ParameterSchemaOrContent::Schema(reference_or) => {
                    visit_schema_ref(reference_or, queue);
                }
                ParameterSchemaOrContent::Content(index_map) => {
                    for mt in index_map.values() {
                        visit_media_type(mt, queue);
                    }
                }
            },
        }
    }
    fn visit_media_type(mt: &MediaType, queue: &mut VecDeque<ComponentRef>) {
        if let Some(schema) = &mt.schema {
            visit_schema_ref(schema, queue);
        }
    }
}
