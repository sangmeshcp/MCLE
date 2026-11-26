use crate::manifest::root;
use anyhow::{bail, Result};

fn default_format(cloud: &str) -> &'static str {
    match cloud {
        "aws" => "smithy",
        "gcp" => "discovery",
        _ => "openapi",
    }
}

fn stub_spec(format: &str, service: &str) -> String {
    match format {
        "smithy" => format!(
            "{{\n  \"smithy\": \"2.0\",\n  \"shapes\": {{\n    \"com.amazonaws.{service}#{service}\": {{\n      \"type\": \"service\",\n      \"version\": \"TODO\",\n      \"operations\": [],\n      \"traits\": {{ \"aws.protocols#awsJson1_1\": {{}}, \"aws.api#service\": {{ \"arnNamespace\": \"{service}\" }}, \"aws.auth#sigv4\": {{ \"name\": \"{service}\" }} }}\n    }}\n  }}\n}}\n"
        ),
        "discovery" => format!("{{\n  \"name\": \"{service}\",\n  \"version\": \"v1\",\n  \"resources\": {{}}\n}}\n"),
        _ => "{\n  \"openapi\": \"3.0.0\",\n  \"info\": { \"title\": \"TODO\", \"version\": \"TODO\" },\n  \"paths\": {}\n}\n".to_string(),
    }
}

pub fn run(cloud: &str, service: &str, tier: &str, format: Option<&str>) -> Result<()> {
    if !service
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        bail!("service name must be lowercase [a-z0-9-]");
    }
    if !root()
        .join("clouds")
        .join(cloud)
        .join("Cargo.toml")
        .exists()
    {
        bail!("unknown cloud `{cloud}` (add a CloudAdapter crate under clouds/ first — see CONTRIBUTING.md)");
    }
    let dir = root().join("services").join(cloud).join(service);
    if dir.exists() {
        bail!("{} already exists", dir.display());
    }
    let format = format.unwrap_or(default_format(cloud));
    let spec_rel = format!("specs/{cloud}/api/{service}.json");
    let spec = root().join(&spec_rel);
    if !spec.exists() {
        std::fs::create_dir_all(spec.parent().unwrap_or(&root()))?;
        std::fs::write(&spec, stub_spec(format, service))?;
        println!("new-service: wrote stub spec {spec_rel} — replace it with the pinned upstream spec and record it in specs/specs.lock");
    }
    let protocol = match cloud {
        "azure" => "protocol = \"arm\"\n",
        "oci" => "protocol = \"ociRest\"\n",
        "ibm" => "protocol = \"ibmRest\"\n",
        _ => "",
    };
    let krate = format!("mcle-svc-{cloud}-{service}");
    let modname = krate.replace('-', "_");
    std::fs::create_dir_all(dir.join("src"))?;
    std::fs::create_dir_all(dir.join("tests"))?;
    std::fs::write(
        dir.join("service.toml"),
        format!(
            "[service]\ncloud = \"{cloud}\"\nname = \"{service}\"\ndisplay = \"{service}\"\ntier = \"{tier}\"\nengines = [\"crud\"]\nowners = [\"@mcle/{cloud}\"]\nfacade = \"generic\"\n{protocol}\n[service.api]\nsource = \"TODO: upstream repo\"\nformat = \"{format}\"\nspec = \"{spec_rel}\"\nversion = \"TODO\"\n\n[defaults]\nlevel = \"L1\"\nplane = \"control\"
"
        ),
    )?;
    std::fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"{krate}\"\ndescription = \"MCLE facade: {cloud} {service}.\"\nversion.workspace = true\nedition.workspace = true\nlicense.workspace = true\npublish.workspace = true\n\n[dependencies]\nmcle-engine-crud.workspace = true\nmcle-sdk.workspace = true\n\n[dev-dependencies]\nserde_json.workspace = true\nmcle-cloud-{cloud}.workspace = true\nmcle-testkit.workspace = true\ntokio.workspace = true\n\n[lints]\nworkspace = true\n"
        ),
    )?;
    std::fs::write(
        dir.join("src").join("lib.rs"),
        "#![forbid(unsafe_code)]\n\n#[path = \"../gen/mod.rs\"]\npub mod gen;\n\nuse std::sync::Arc;\nuse mcle_engine_crud::GenericCrudFacade;\nuse mcle_sdk::{CloudAdapter, Host, ServiceFacade};\n\npub fn facade(host: &Host, adapter: &dyn CloudAdapter) -> Arc<dyn ServiceFacade> {\n    let shaper = adapter\n        .crud_shaper(gen::MANIFEST.protocol)\n        .unwrap_or_else(|| Arc::new(mcle_engine_crud::NoShaper));\n    Arc::new(GenericCrudFacade::new(host, &gen::MANIFEST, &gen::ROUTES, &gen::OPS, &gen::CRUD_RESOURCES, shaper))\n}\n",
    )?;
    std::fs::write(
        dir.join("tests").join("l1.rs"),
        format!(
            "use mcle_testkit::TestServer;\n\n#[tokio::test]\nasync fn starts_with_service_registered() {{\n    let srv = TestServer::start(|host| {{\n        let adapter = mcle_cloud_{cloud}::adapter(host);\n        (vec![{modname}::facade(host, &*adapter)], vec![adapter])\n    }})\n    .await;\n    let info = srv.info().await;\n    assert!(info[\"services\"].to_string().contains(\"{service}\"));\n}}\n"
        ),
    )?;
    println!("new-service: created services/{cloud}/{service}");
    crate::gen::run(false)?;
    crate::bundle::run(false)?;
    crate::status::run(false)?;
    println!("next: declare [crud.resources] in service.toml, run `cargo xtask codegen`, then `cargo test -p {krate}`");
    Ok(())
}
