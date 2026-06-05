// Asset (logo / favicon) handling for the CLI.

use std::path::Path;

/// Copy `src` into `<output>/images/<basename>`, creating the directory
/// if needed. Returns the basename so callers can build a docfx-relative
/// reference (e.g. "images/logo.svg").
pub(crate) fn copy_asset_to_images(output: &Path, src: &Path, kind: &str) -> Option<String> {
    let file_name = match src.file_name().and_then(|s| s.to_str()) {
        Some(n) => n.to_string(),
        None => {
            eprintln!(
                "warning: --{kind} path {} has no filename, skipping copy",
                src.display()
            );
            return None;
        }
    };
    let images_dir = output.join("images");
    if let Err(e) = std::fs::create_dir_all(&images_dir) {
        eprintln!(
            "warning: cannot create {}: {e} (skipping --{kind})",
            images_dir.display()
        );
        return None;
    }
    let dest = images_dir.join(&file_name);
    match std::fs::copy(src, &dest) {
        Ok(_) => {
            eprintln!("copied {} → {}", src.display(), dest.display());
            Some(format!("images/{file_name}"))
        }
        Err(e) => {
            eprintln!(
                "warning: cannot copy {} → {}: {e}",
                src.display(),
                dest.display()
            );
            None
        }
    }
}

/// Copy logo/favicon assets (if any) into `<output>/images/` and print
/// the matching docfx `globalMetadata` snippet when --docfx is set.
pub(crate) fn handle_branding_assets(
    output: &Path,
    logo: Option<&Path>,
    favicon: Option<&Path>,
    docfx: bool,
) {
    let logo_rel = logo.and_then(|p| copy_asset_to_images(output, p, "logo"));
    let favicon_rel = favicon.and_then(|p| copy_asset_to_images(output, p, "favicon"));

    if !docfx || (logo_rel.is_none() && favicon_rel.is_none()) {
        return;
    }

    eprintln!();
    eprintln!("docfx: add the following to docfx.json `globalMetadata` (paths are");
    eprintln!("docfx: relative to the generated site root):");
    if let Some(p) = &logo_rel {
        eprintln!("  \"_appLogoPath\": \"{p}\",");
    }
    if let Some(p) = &favicon_rel {
        eprintln!("  \"_appFaviconPath\": \"{p}\",");
    }
}
