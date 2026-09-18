//! Local, fixture-only native verification. Default is read-only.
#[cfg(target_os = "macos")]
fn main() {
    use platform_macos::login::{inspect_adapter, Cancellation, ProbePhase};
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !args.is_empty() && args != ["--write-fixture"] {
        eprintln!("Usage: qq_ax_probe [--write-fixture]");
        std::process::exit(2);
    }
    let adapter = autologin_core::adapter::builtin_for_application(
        autologin_core::Platform::Macos,
        "com.tencent.qq",
    )
    .expect("valid builtin adapter")
    .expect("QQ adapter");
    match inspect_adapter(&adapter) {
        Ok(probe) => {
            println!("Read-only AX metadata: {:?}", probe.summary());
            if !args.is_empty() {
                let report = probe.exercise_fixture(&Cancellation::default());
                println!("Fixture result (AX acknowledgement only): {report:?}");
                if report.phase != ProbePhase::Complete {
                    eprintln!(
                        "Stopped. Clear any remaining fixture text manually; no automatic retry."
                    );
                    std::process::exit(1);
                }
            }
        }
        Err(error) => {
            eprintln!("Native AX probe unavailable: {error:?}");
            std::process::exit(1);
        }
    }
}
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("This probe requires macOS.");
    std::process::exit(1);
}
