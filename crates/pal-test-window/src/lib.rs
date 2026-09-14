use std::{fmt, time::Duration};

#[cfg(windows)]
mod windows_host;

pub const MIN_WIDTH: u32 = 640;
pub const MIN_HEIGHT: u32 = 360;
pub const MAX_WIDTH: u32 = 7_680;
pub const MAX_HEIGHT: u32 = 4_320;
pub const MAX_DURATION_SECONDS: u64 = 120;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphicsApi {
    Gdi,
    DirectX11,
    DirectX12,
}

impl GraphicsApi {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Gdi => "gdi",
            Self::DirectX11 => "dx11",
            Self::DirectX12 => "dx12",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "gdi" => Some(Self::Gdi),
            "dx11" => Some(Self::DirectX11),
            "dx12" => Some(Self::DirectX12),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowMode {
    Windowed,
    Borderless,
    Exclusive,
}

impl WindowMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Windowed => "windowed",
            Self::Borderless => "borderless",
            Self::Exclusive => "exclusive",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "windowed" => Some(Self::Windowed),
            "borderless" => Some(Self::Borderless),
            "exclusive" => Some(Self::Exclusive),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TestWindowOptions {
    pub graphics_api: GraphicsApi,
    pub window_mode: WindowMode,
    pub width: u32,
    pub height: u32,
    pub duration: Duration,
    pub allow_exclusive_fullscreen: bool,
}

impl Default for TestWindowOptions {
    fn default() -> Self {
        Self {
            graphics_api: GraphicsApi::Gdi,
            window_mode: WindowMode::Windowed,
            width: 720,
            height: 480,
            duration: Duration::from_secs(120),
            allow_exclusive_fullscreen: false,
        }
    }
}

impl TestWindowOptions {
    pub fn parse<I, S>(arguments: I) -> Result<Self, OptionsError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut options = Self::default();
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            match argument.as_ref() {
                "--api" => {
                    let value = next_value(&mut arguments, "--api")?;
                    options.graphics_api = GraphicsApi::parse(value.as_ref()).ok_or_else(|| {
                        OptionsError::InvalidValue {
                            option: "--api",
                            value: value.as_ref().to_owned(),
                        }
                    })?;
                }
                "--window-mode" => {
                    let value = next_value(&mut arguments, "--window-mode")?;
                    options.window_mode = WindowMode::parse(value.as_ref()).ok_or_else(|| {
                        OptionsError::InvalidValue {
                            option: "--window-mode",
                            value: value.as_ref().to_owned(),
                        }
                    })?;
                }
                "--width" => {
                    let value = next_value(&mut arguments, "--width")?;
                    options.width =
                        parse_bounded_u32("--width", value.as_ref(), MIN_WIDTH, MAX_WIDTH)?;
                }
                "--height" => {
                    let value = next_value(&mut arguments, "--height")?;
                    options.height =
                        parse_bounded_u32("--height", value.as_ref(), MIN_HEIGHT, MAX_HEIGHT)?;
                }
                "--duration-seconds" => {
                    let value = next_value(&mut arguments, "--duration-seconds")?;
                    let seconds = parse_bounded_u64(
                        "--duration-seconds",
                        value.as_ref(),
                        1,
                        MAX_DURATION_SECONDS,
                    )?;
                    options.duration = Duration::from_secs(seconds);
                }
                "--allow-exclusive-fullscreen" => {
                    options.allow_exclusive_fullscreen = true;
                }
                unknown => return Err(OptionsError::UnknownArgument(unknown.to_owned())),
            }
        }

        if options.window_mode == WindowMode::Exclusive {
            if !options.allow_exclusive_fullscreen {
                return Err(OptionsError::ExclusiveOptInRequired);
            }
            if options.graphics_api == GraphicsApi::Gdi {
                return Err(OptionsError::ExclusiveRequiresDirectX);
            }
        }
        Ok(options)
    }

    pub fn usage() -> &'static str {
        "Palworld-Win64-Shipping [--api gdi|dx11|dx12] \
         [--window-mode windowed|borderless|exclusive] \
         [--width 640..7680] [--height 360..4320] \
         [--duration-seconds 1..120] [--allow-exclusive-fullscreen]"
    }
}

#[cfg(windows)]
pub fn run_from_args<I, S>(arguments: I) -> Result<(), Box<dyn std::error::Error>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let options = TestWindowOptions::parse(arguments).map_err(|error| {
        eprintln!("{error}");
        eprintln!("{}", TestWindowOptions::usage());
        error
    })?;
    windows_host::run(options)
}

#[cfg(not(windows))]
pub fn run_from_args<I, S>(_arguments: I) -> Result<(), Box<dyn std::error::Error>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    Err("Palworld test window requires Windows".into())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OptionsError {
    MissingValue(&'static str),
    InvalidValue { option: &'static str, value: String },
    UnknownArgument(String),
    ExclusiveOptInRequired,
    ExclusiveRequiresDirectX,
}

impl fmt::Display for OptionsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingValue(option) => write!(formatter, "{option} requires a value"),
            Self::InvalidValue { option, value } => {
                write!(formatter, "{option} has invalid value {value:?}")
            }
            Self::UnknownArgument(argument) => write!(formatter, "unknown argument {argument:?}"),
            Self::ExclusiveOptInRequired => write!(
                formatter,
                "exclusive mode requires --allow-exclusive-fullscreen"
            ),
            Self::ExclusiveRequiresDirectX => {
                formatter.write_str("exclusive mode requires --api dx11 or --api dx12")
            }
        }
    }
}

impl std::error::Error for OptionsError {}

fn next_value<I, S>(arguments: &mut I, option: &'static str) -> Result<S, OptionsError>
where
    I: Iterator<Item = S>,
{
    arguments.next().ok_or(OptionsError::MissingValue(option))
}

fn parse_bounded_u32(
    option: &'static str,
    value: &str,
    minimum: u32,
    maximum: u32,
) -> Result<u32, OptionsError> {
    let parsed = value
        .parse::<u32>()
        .map_err(|_| OptionsError::InvalidValue {
            option,
            value: value.to_owned(),
        })?;
    if !(minimum..=maximum).contains(&parsed) {
        return Err(OptionsError::InvalidValue {
            option,
            value: value.to_owned(),
        });
    }
    Ok(parsed)
}

fn parse_bounded_u64(
    option: &'static str,
    value: &str,
    minimum: u64,
    maximum: u64,
) -> Result<u64, OptionsError> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| OptionsError::InvalidValue {
            option,
            value: value.to_owned(),
        })?;
    if !(minimum..=maximum).contains(&parsed) {
        return Err(OptionsError::InvalidValue {
            option,
            value: value.to_owned(),
        });
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_preserve_the_original_gdi_test_window() {
        let options = TestWindowOptions::parse(std::iter::empty::<&str>()).unwrap();

        assert_eq!(options, TestWindowOptions::default());
    }

    #[test]
    fn selects_dx11_borderless_with_bounded_dimensions() {
        let options = TestWindowOptions::parse([
            "--api",
            "dx11",
            "--window-mode",
            "borderless",
            "--width",
            "1920",
            "--height",
            "1080",
            "--duration-seconds",
            "10",
        ])
        .unwrap();

        assert_eq!(options.graphics_api, GraphicsApi::DirectX11);
        assert_eq!(options.window_mode, WindowMode::Borderless);
        assert_eq!((options.width, options.height), (1920, 1080));
        assert_eq!(options.duration, Duration::from_secs(10));
    }

    #[test]
    fn selects_dx12_exclusive_only_with_explicit_opt_in() {
        assert_eq!(
            TestWindowOptions::parse(["--api", "dx12", "--window-mode", "exclusive",]),
            Err(OptionsError::ExclusiveOptInRequired)
        );

        let options = TestWindowOptions::parse([
            "--api",
            "dx12",
            "--window-mode",
            "exclusive",
            "--allow-exclusive-fullscreen",
        ])
        .unwrap();
        assert_eq!(options.graphics_api, GraphicsApi::DirectX12);
        assert_eq!(options.window_mode, WindowMode::Exclusive);
    }

    #[test]
    fn rejects_unsafe_or_unsupported_boundaries() {
        assert!(matches!(
            TestWindowOptions::parse(["--width", "639"]),
            Err(OptionsError::InvalidValue {
                option: "--width",
                ..
            })
        ));
        assert_eq!(
            TestWindowOptions::parse([
                "--api",
                "gdi",
                "--window-mode",
                "exclusive",
                "--allow-exclusive-fullscreen",
            ]),
            Err(OptionsError::ExclusiveRequiresDirectX)
        );
        assert_eq!(
            TestWindowOptions::parse(["--unknown"]),
            Err(OptionsError::UnknownArgument("--unknown".to_owned()))
        );
    }
}
