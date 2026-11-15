//! Per-process compatibility policy; never mutate the session or driver environment.
use eframe::{egui_wgpu::WgpuSetup, wgpu};

pub struct Configuration {
    pub wgpu: eframe::WgpuConfiguration,
    pub event_loop_builder: Option<eframe::EventLoopBuilderHook>,
}

#[derive(Debug, PartialEq, Eq)]
enum WindowSystem {
    X11,
    Wayland,
    Auto,
}

fn window_system(value: Option<&str>, has_display: bool) -> Result<WindowSystem, &'static str> {
    match value {
        None if has_display => Ok(WindowSystem::X11),
        None | Some("auto") => Ok(WindowSystem::Auto),
        Some("x11") => Ok(WindowSystem::X11),
        Some("wayland") => Ok(WindowSystem::Wayland),
        Some(_) => Err("MFALMION_WINDOW_SYSTEM must be x11, wayland, or auto"),
    }
}

pub fn configuration() -> eframe::Result<Configuration> {
    let mut wgpu = eframe::WgpuConfiguration::default();
    if let WgpuSetup::CreateNew(setup) = &mut wgpu.wgpu_setup {
        // A shell does not need the discrete GPU. Respect explicit diagnostic overrides.
        setup.power_preference =
            wgpu::PowerPreference::from_env().unwrap_or(wgpu::PowerPreference::LowPower);
        setup.instance_descriptor.backends =
            wgpu::Backends::from_env().unwrap_or(wgpu::Backends::VULKAN);
    }
    let event_loop_builder = None;
    #[cfg(target_os = "linux")]
    let event_loop_builder = {
        use winit::platform::{wayland::EventLoopBuilderExtWayland, x11::EventLoopBuilderExtX11};
        let requested = std::env::var("MFALMION_WINDOW_SYSTEM").ok();
        let has_display = std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty());
        let system = window_system(requested.as_deref(), has_display).map_err(|message| {
            eframe::Error::AppCreation(
                std::io::Error::new(std::io::ErrorKind::InvalidInput, message).into(),
            )
        })?;
        eprintln!(
            "level=INFO event=graphics_policy window_system={system:?} session={:?} display={:?} wayland_display={:?}",
            std::env::var_os("XDG_SESSION_TYPE"),
            std::env::var_os("DISPLAY"),
            std::env::var_os("WAYLAND_DISPLAY")
        );
        match system {
            WindowSystem::X11 => Some(Box::new(
                |builder: &mut eframe::EventLoopBuilder<eframe::UserEvent>| {
                    builder.with_x11();
                },
            ) as eframe::EventLoopBuilderHook),
            WindowSystem::Wayland => Some(Box::new(
                |builder: &mut eframe::EventLoopBuilder<eframe::UserEvent>| {
                    builder.with_wayland();
                },
            ) as eframe::EventLoopBuilderHook),
            WindowSystem::Auto => event_loop_builder,
        }
    };
    Ok(Configuration {
        wgpu,
        event_loop_builder,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_default_and_overrides() {
        assert_eq!(window_system(None, true), Ok(WindowSystem::X11));
        assert_eq!(window_system(None, false), Ok(WindowSystem::Auto));
        assert_eq!(
            window_system(Some("wayland"), true),
            Ok(WindowSystem::Wayland)
        );
        assert_eq!(window_system(Some("x11"), false), Ok(WindowSystem::X11));
        assert_eq!(window_system(Some("auto"), true), Ok(WindowSystem::Auto));
        assert!(window_system(Some("typo"), true).is_err());
    }
}
