# Bundled 7-Zip Unpacker

Acumod uses this directory for its bundled archive extraction backend.

Bundled Windows x64 files:

- `7z.exe`
- `7z.dll`
- `License.txt`

Current bundled version: 7-Zip 26.02 x64.

`7za.exe` is not enough for Acumod because the standalone 7-Zip console build does not support RAR extraction. Keep the 7-Zip license text beside the binaries and make sure release notes mention the bundled LGPL / unRAR-restricted component.

The source for official 7-Zip downloads is:

https://www.7-zip.org/download.html
