// Windows service registration
// Handles installing/uninstalling ptree-driver as a Windows service

use crate::error::{DriverError, DriverResult};
use log::info;
use std::path::PathBuf;

#[cfg(windows)]
use winapi::um::winsvc::*;
#[cfg(windows)]
use winapi::um::handleapi::CloseHandle;
#[cfg(windows)]
use std::ffi::CString;

// Windows service constants
#[cfg(windows)]
const SERVICE_WIN32_OWN_PROCESS: u32 = 0x0010;
#[cfg(windows)]
const SERVICE_AUTO_START: u32 = 0x0002;
#[cfg(windows)]
const SERVICE_ERROR_NORMAL: u32 = 0x0001;

/// Service metadata
pub const SERVICE_NAME: &str = "PTreeDriver";
pub const SERVICE_DISPLAY_NAME: &str = "ptree File System Driver";
pub const SERVICE_DESCRIPTION: &str = "Monitors NTFS file system changes via USN Journal for incremental cache updates";

/// Register ptree-driver as a Windows service
#[cfg(windows)]
pub fn register_service(executable_path: &PathBuf) -> DriverResult<()> {
    info!("Registering ptree-driver service");
    
    // Verify executable exists
    if !executable_path.exists() {
        return Err(DriverError::Windows(
            format!("Executable not found: {:?}", executable_path)
        ));
    }

    // Convert path to Windows format
    let exe_path = executable_path
        .to_str()
        .ok_or_else(|| DriverError::Windows("Invalid executable path".to_string()))?;

    // Open Service Control Manager
    let scm_handle = unsafe {
        OpenSCManagerA(
            std::ptr::null(),
            std::ptr::null(),
            SC_MANAGER_ALL_ACCESS,
        )
    };

    if scm_handle.is_null() {
        return Err(DriverError::Windows(
            format!("Failed to open Service Control Manager: {}", std::io::Error::last_os_error())
        ));
    }

    // Create service
    let service_name = CString::new(SERVICE_NAME)
        .map_err(|_| DriverError::Windows("Invalid service name".to_string()))?;
    let display_name = CString::new(SERVICE_DISPLAY_NAME)
        .map_err(|_| DriverError::Windows("Invalid display name".to_string()))?;
    let exe_path_cstr = CString::new(format!("\"{}\" run", exe_path))
        .map_err(|_| DriverError::Windows("Invalid executable path".to_string()))?;

    let service_handle = unsafe {
        CreateServiceA(
            scm_handle,
            service_name.as_ptr(),
            display_name.as_ptr(),
            SERVICE_ALL_ACCESS,
            SERVICE_WIN32_OWN_PROCESS,
            SERVICE_AUTO_START,
            SERVICE_ERROR_NORMAL,
            exe_path_cstr.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
        )
    };

    unsafe { CloseHandle(scm_handle as *mut _) };

    if service_handle.is_null() {
        let error = std::io::Error::last_os_error();
        // Service might already exist
        if error.raw_os_error() == Some(1073) { // ERROR_SERVICE_EXISTS
            info!("Service already registered");
            return Ok(());
        }
        return Err(DriverError::Windows(
            format!("Failed to create service: {}", error)
        ));
    }

    unsafe { CloseHandle(service_handle as *mut _) };

    info!("Service registered successfully");
    info!("Service name: {}", SERVICE_NAME);
    info!("Service will start automatically on next boot");

    Ok(())
}

/// Unregister ptree-driver service
#[cfg(windows)]
pub fn unregister_service() -> DriverResult<()> {
    info!("Unregistering ptree-driver service");

    let scm_handle = unsafe {
        OpenSCManagerA(
            std::ptr::null(),
            std::ptr::null(),
            SC_MANAGER_ALL_ACCESS,
        )
    };

    if scm_handle.is_null() {
        return Err(DriverError::Windows(
            format!("Failed to open Service Control Manager: {}", std::io::Error::last_os_error())
        ));
    }

    let service_name = CString::new(SERVICE_NAME)
        .map_err(|_| DriverError::Windows("Invalid service name".to_string()))?;

    let service_handle = unsafe {
        OpenServiceA(
            scm_handle,
            service_name.as_ptr(),
            SERVICE_ALL_ACCESS,
        )
    };

    if service_handle.is_null() {
        unsafe { CloseHandle(scm_handle as *mut _) };
        return Err(DriverError::Windows(
            "Service not found".to_string()
        ));
    }

    // Stop the service first
    let mut service_status = unsafe { std::mem::zeroed::<SERVICE_STATUS>() };
    unsafe {
        ControlService(
            service_handle,
            SERVICE_CONTROL_STOP,
            &mut service_status,
        );
    }

    // Delete the service
    let result = unsafe {
        DeleteService(service_handle)
    };

    unsafe {
        CloseHandle(service_handle as *mut _);
        CloseHandle(scm_handle as *mut _);
    }

    if result == 0 {
        return Err(DriverError::Windows(
            format!("Failed to delete service: {}", std::io::Error::last_os_error())
        ));
    }

    info!("Service unregistered successfully");
    Ok(())
}

/// Start the service
#[cfg(windows)]
pub fn start_service() -> DriverResult<()> {
    info!("Starting ptree-driver service");

    let scm_handle = unsafe {
        OpenSCManagerA(
            std::ptr::null(),
            std::ptr::null(),
            SC_MANAGER_ALL_ACCESS,
        )
    };

    if scm_handle.is_null() {
        return Err(DriverError::Windows(
            "Failed to open Service Control Manager".to_string()
        ));
    }

    let service_name = CString::new(SERVICE_NAME)
        .map_err(|_| DriverError::Windows("Invalid service name".to_string()))?;

    let service_handle = unsafe {
        OpenServiceA(
            scm_handle,
            service_name.as_ptr(),
            SERVICE_START,
        )
    };

    if service_handle.is_null() {
        unsafe { CloseHandle(scm_handle as *mut _) };
        return Err(DriverError::Windows(
            "Service not found".to_string()
        ));
    }

    let result = unsafe {
        StartServiceA(
            service_handle,
            0,
            std::ptr::null_mut(),
        )
    };

    unsafe {
        CloseHandle(service_handle as *mut _);
        CloseHandle(scm_handle as *mut _);
    }

    if result == 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(1056) { // ERROR_SERVICE_ALREADY_RUNNING
            info!("Service is already running");
            return Ok(());
        }
        return Err(DriverError::Windows(
            format!("Failed to start service: {}", error)
        ));
    }

    info!("Service started successfully");
    Ok(())
}

/// Stop the service
#[cfg(windows)]
pub fn stop_service() -> DriverResult<()> {
    info!("Stopping ptree-driver service");

    let scm_handle = unsafe {
        OpenSCManagerA(
            std::ptr::null(),
            std::ptr::null(),
            SC_MANAGER_ALL_ACCESS,
        )
    };

    if scm_handle.is_null() {
        return Err(DriverError::Windows(
            "Failed to open Service Control Manager".to_string()
        ));
    }

    let service_name = CString::new(SERVICE_NAME)
        .map_err(|_| DriverError::Windows("Invalid service name".to_string()))?;

    let service_handle = unsafe {
        OpenServiceA(
            scm_handle,
            service_name.as_ptr(),
            SERVICE_STOP,
        )
    };

    if service_handle.is_null() {
        unsafe { CloseHandle(scm_handle as *mut _) };
        return Err(DriverError::Windows(
            "Service not found".to_string()
        ));
    }

    let mut service_status = unsafe { std::mem::zeroed::<SERVICE_STATUS>() };
    let result = unsafe {
        ControlService(
            service_handle,
            SERVICE_CONTROL_STOP,
            &mut service_status,
        )
    };

    unsafe {
        CloseHandle(service_handle as *mut _);
        CloseHandle(scm_handle as *mut _);
    }

    if result == 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(1062) { // ERROR_SERVICE_NOT_ACTIVE
            info!("Service is not running");
            return Ok(());
        }
        return Err(DriverError::Windows(
            format!("Failed to stop service: {}", error)
        ));
    }

    info!("Service stopped successfully");
    Ok(())
}

/// Non-Windows stubs
#[cfg(not(windows))]
pub fn register_service(_executable_path: &PathBuf) -> DriverResult<()> {
    Err(DriverError::Windows(
        "Service registration not supported on non-Windows platforms".to_string()
    ))
}

#[cfg(not(windows))]
pub fn unregister_service() -> DriverResult<()> {
    Err(DriverError::Windows(
        "Service unregistration not supported on non-Windows platforms".to_string()
    ))
}

#[cfg(not(windows))]
pub fn start_service() -> DriverResult<()> {
    Err(DriverError::Windows(
        "Service start not supported on non-Windows platforms".to_string()
    ))
}

#[cfg(not(windows))]
pub fn stop_service() -> DriverResult<()> {
    Err(DriverError::Windows(
        "Service stop not supported on non-Windows platforms".to_string()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_constants() {
        assert!(!SERVICE_NAME.is_empty());
        assert!(!SERVICE_DISPLAY_NAME.is_empty());
    }
}
