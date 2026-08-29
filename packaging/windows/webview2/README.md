# WebView2 policy

The NSIS config uses Microsoft's Evergreen bootstrapper so a clean Windows 10/11 installation can acquire the supported WebView2 runtime. The WebView carries only the control UI; continuous PCM and video remain in native processes.

For air-gapped installer qualification, stage Microsoft's matching Evergreen Standalone Installer in the private build environment and use a reviewed local config overlay. Do not commit the redistributable or replace the Microsoft source with an untrusted mirror.

The manual clean-machine-style lifecycle check is implemented by `scripts/installer-smoke.ps1`. It is intentionally excluded from push and pull-request CI because it performs a real current-user install/uninstall. The only GitHub workflow that invokes it is the explicit `workflow_dispatch` package smoke job, and that workflow does not upload or publish the installer.
