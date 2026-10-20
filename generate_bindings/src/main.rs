use interoptopus_backend_csharp::{Config, Generator};
use interoptopus::{Error, Interop};

fn main() -> Result<(), Error> {
    // Get the ffi_inventory from our library
    let inventory = unity_native_ble::ffi_inventory();

    // Configure the C# backend
    let config = Config {
        dll_name: "unity_native_ble".to_string(),
        ..Config::default()
    };

    // Generate bindings
    let generator = Generator::new(config, inventory.clone());
    generator.write_file("../UnityNativeBLE.generated.cs")?;

    println!("✅ C# bindings generated to: UnityNativeBLE.generated.cs");
    println!("📝 Total functions exported: {}", inventory.functions().len());
    println!("📦 Total types exported: {}", inventory.ctypes().len());

    Ok(())
}