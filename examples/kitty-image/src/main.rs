//! Launches the Kitty graphics example with a configurable capability policy.

use std::{
    env,
    error::Error,
    ffi::{OsStr, OsString},
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

use arborui::{
    CrosstermBackend, KittyGraphicsMode, RgbaImage, TerminalBackend, TerminalState, run,
    terminal::MouseMode,
};
use arborui_example_kitty_image::KittyImageDemo;

const MAX_DIRECTORY_IMAGES: usize = 256;
const MAX_DIRECTORY_IMAGE_BYTES: usize = 256 * 1024 * 1024;
const DEFAULT_IMAGE_PATH: &str = "./images";

type NamedImage = (String, RgbaImage);

fn main() -> Result<(), Box<dyn Error>> {
    let Some(options) = launch_options()? else {
        return Ok(());
    };
    let backend = CrosstermBackend::new(io::stdout())?.with_kitty_graphics(options.graphics_mode);
    let viewport = backend.viewport()?;
    let geometry = viewport.pixels.map_or_else(
        || "cell aspect: 2:1 fallback".to_owned(),
        |pixels| {
            format!(
                "viewport: {}x{} cells / {}x{} px",
                viewport.cells.width, viewport.cells.height, pixels.width, pixels.height
            )
        },
    );
    let status = format!(
        "Policy: {}; Kitty graphics: {}; {geometry}",
        options.graphics_policy,
        if backend.capabilities().kitty_graphics {
            "enabled"
        } else {
            "disabled"
        }
    );
    let mut terminal_state = TerminalState::fullscreen();
    terminal_state.mouse = MouseMode::Capture;
    terminal_state.title = Some("ArborUI Kitty Image Lab".to_owned());
    let (first, additional) = load_image_sources(&options.image)?;
    let application = KittyImageDemo::with_images(status, viewport, first, additional)?;

    run(
        application,
        backend,
        terminal_state,
        Duration::from_millis(16),
    )?;
    Ok(())
}

struct LaunchOptions {
    graphics_mode: KittyGraphicsMode,
    graphics_policy: &'static str,
    image: PathBuf,
}

fn launch_options() -> io::Result<Option<LaunchOptions>> {
    launch_options_from(env::args_os().skip(1))
}

fn launch_options_from(
    arguments: impl IntoIterator<Item = OsString>,
) -> io::Result<Option<LaunchOptions>> {
    let mut graphics_mode = KittyGraphicsMode::Auto;
    let mut graphics_policy = "auto";
    let mut image = None;
    let mut mode_selected = false;
    let mut positional_only = false;
    for argument in arguments {
        if !positional_only && argument == OsStr::new("--") {
            positional_only = true;
            continue;
        }
        if !positional_only && matches!(argument.to_str(), Some("--help" | "-h")) {
            println!(
                "Usage: arborui-example-kitty-image [--auto|--kitty|--no-kitty] [IMAGE_OR_DIRECTORY]\nDefault: {DEFAULT_IMAGE_PATH}"
            );
            return Ok(None);
        }
        let mode = if positional_only {
            None
        } else {
            match argument.to_str() {
                Some("--auto") => Some((KittyGraphicsMode::Auto, "auto")),
                Some("--kitty") => Some((KittyGraphicsMode::Enabled, "forced enabled")),
                Some("--no-kitty") => Some((KittyGraphicsMode::Disabled, "forced disabled")),
                _ => None,
            }
        };
        if let Some((selected_mode, selected_policy)) = mode {
            if mode_selected {
                return Err(invalid_arguments(
                    "graphics mode was supplied more than once",
                ));
            }
            mode_selected = true;
            graphics_mode = selected_mode;
            graphics_policy = selected_policy;
            continue;
        }
        if !positional_only && argument.to_string_lossy().starts_with('-') {
            return Err(invalid_arguments(format!(
                "unknown option {:?}",
                argument.to_string_lossy()
            )));
        }
        if image.replace(PathBuf::from(argument)).is_some() {
            return Err(invalid_arguments("expected at most one image path"));
        }
    }
    Ok(Some(LaunchOptions {
        graphics_mode,
        graphics_policy,
        image: image.unwrap_or_else(|| PathBuf::from(DEFAULT_IMAGE_PATH)),
    }))
}

fn invalid_arguments(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn load_image_sources(path: &Path) -> Result<(NamedImage, Vec<NamedImage>), Box<dyn Error>> {
    let mut paths = if path.is_dir() {
        let mut paths = Vec::new();
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            if entry.file_type()?.is_file() && is_supported_image_path(&entry.path()) {
                paths.push(entry.path());
            }
        }
        paths.sort();
        paths
    } else {
        vec![path.to_owned()]
    };

    if paths.len() > MAX_DIRECTORY_IMAGES {
        return Err(invalid_arguments(format!(
            "{} contains more than {MAX_DIRECTORY_IMAGES} supported image files",
            path.display()
        ))
        .into());
    }
    let Some(first_path) = paths.first().cloned() else {
        return Err(invalid_arguments(format!(
            "{} contains no supported image files",
            path.display()
        ))
        .into());
    };
    paths.remove(0);

    let first = load_named_image(&first_path)?;
    let mut total_bytes = first.1.pixels().len();
    let mut additional = Vec::with_capacity(paths.len());
    for path in paths {
        let image = load_named_image(&path)?;
        total_bytes = total_bytes
            .checked_add(image.1.pixels().len())
            .ok_or_else(|| invalid_arguments("decoded directory image size overflow"))?;
        if total_bytes > MAX_DIRECTORY_IMAGE_BYTES {
            return Err(invalid_arguments(format!(
                "decoded directory images exceed the {MAX_DIRECTORY_IMAGE_BYTES}-byte limit"
            ))
            .into());
        }
        additional.push(image);
    }
    Ok((first, additional))
}

fn load_named_image(path: &Path) -> Result<NamedImage, Box<dyn Error>> {
    let image = arborui::image_decoder::load(path).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to decode {}: {error}", path.display()),
        )
    })?;
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    Ok((name, image))
}

fn is_supported_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "bmp"
                    | "gif"
                    | "ico"
                    | "jpeg"
                    | "jpg"
                    | "pam"
                    | "pbm"
                    | "pgm"
                    | "png"
                    | "pnm"
                    | "ppm"
                    | "qoi"
                    | "tga"
                    | "tif"
                    | "tiff"
                    | "webp"
            )
        })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn omitted_path_defaults_to_images_directory() -> io::Result<()> {
        let Some(options) = launch_options_from(std::iter::empty())? else {
            panic!("empty arguments must launch the application");
        };

        assert_eq!(options.image, Path::new(DEFAULT_IMAGE_PATH));
        Ok(())
    }

    #[test]
    fn directory_images_are_filtered_and_sorted() -> Result<(), Box<dyn Error>> {
        let directory = TemporaryDirectory::new()?;
        fs::write(directory.path().join("b.tga"), tga(1, 1, [0, 0, 255, 255]))?;
        fs::write(directory.path().join("a.tga"), tga(1, 1, [255, 0, 0, 255]))?;
        fs::write(directory.path().join("notes.txt"), b"not an image")?;

        let (first, additional) = load_image_sources(directory.path())?;

        assert_eq!(first.0, "a.tga");
        assert_eq!(additional.len(), 1);
        assert_eq!(additional[0].0, "b.tga");
        assert_eq!((first.1.width(), first.1.height()), (1, 1));
        Ok(())
    }

    #[test]
    fn loaded_images_retain_source_resolution() -> Result<(), Box<dyn Error>> {
        let directory = TemporaryDirectory::new()?;
        let path = directory.path().join("large.tga");
        fs::write(&path, tga(500, 500, [255, 0, 0, 255]))?;

        let (_, image) = load_named_image(&path)?;

        assert_eq!((image.width(), image.height()), (500, 500));
        Ok(())
    }

    fn tga(width: u16, height: u16, bgra: [u8; 4]) -> Vec<u8> {
        let mut bytes = vec![0; 18];
        bytes[2] = 2;
        bytes[12..14].copy_from_slice(&width.to_le_bytes());
        bytes[14..16].copy_from_slice(&height.to_le_bytes());
        bytes[16] = 32;
        bytes[17] = 0x28;
        for _ in 0..u32::from(width) * u32::from(height) {
            bytes.extend_from_slice(&bgra);
        }
        bytes
    }

    struct TemporaryDirectory(PathBuf);

    impl TemporaryDirectory {
        fn new() -> io::Result<Self> {
            loop {
                let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
                let path = env::temp_dir().join(format!(
                    "arborui-kitty-image-{}-{sequence}",
                    std::process::id()
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Ok(Self(path)),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error),
                }
            }
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TemporaryDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
