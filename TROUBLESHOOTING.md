# 🔧 QuStudio - Troubleshooting Guide

## Common Issues & Solutions

### **Issue 1: "failed to load manifest for dependency `qu-core`"**

**Error:**
```
Error failed to get cargo metadata: ... 
failed to read `C:\Users\...\engine\crates\qu-core\Cargo.toml`
```

**Solution:**
The paths have been fixed! The crates are now correctly referenced at the root level (`../../qu-core`).

If you still see this error:
```bash
# Clean and rebuild
cd qu-studio-tauri/src-tauri
cargo clean
cargo build
```

---

### **Issue 2: "qu command not found"**

**Error:**
```
Failed to execute Qu code: The system cannot find the file specified.
```

**Solution:**
You need to build/install the Qu CLI first:

```bash
# Option 1: Build (development)
cd engine/crates/qu-cli      # from the repository root
cargo build

# This creates: engine/target/debug/qu.exe

# Option 2: Install (system-wide)
cargo install --path .

# This installs qu.exe to: C:\Users\<you>\.cargo\bin\qu.exe
```

**Verify installation:**
```bash
qu version
# Should output: qu 0.1.0 (or similar)
```

**Add to PATH (if needed):**
```bash
# Add to your PATH environment variable:
%USERPROFILE%\.cargo\bin
```

---

### **Issue 3: npm install fails**

**Error:**
```
npm ERR! code ENOENT
npm ERR! syscall open
```

**Solution:**
```bash
# Clear npm cache
npm cache clean --force

# Delete node_modules and package-lock.json
rm -rf node_modules package-lock.json

# Reinstall
npm install
```

---

### **Issue 4: Tauri CLI not found**

**Error:**
```
command not found: tauri
```

**Solution:**
```bash
# Install Tauri CLI
cargo install tauri-cli

# Verify
tauri --version
```

---

### **Issue 5: Build fails with Rust errors**

**Error:**
```
error[E0432]: unresolved import `...`
```

**Solution:**
```bash
# Update Rust
rustup update

# Clean build
cargo clean
cargo build
```

---

### **Issue 6: App crashes on startup**

**Solution:**
```bash
# Run in debug mode to see errors
cd qu-studio-tauri
npm run tauri dev

# Check console output for errors
# Press F12 in the app to open DevTools
```

---

### **Issue 7: Code execution doesn't work**

**Symptoms:**
- Run button works but nothing happens
- Terminal shows no output

**Solution:**
1. Make sure qu.exe exists:
   ```bash
   ls engine/target/debug/qu.exe
   ```

2. Test qu CLI directly:
   ```bash
   echo "x = 1 to 10" | qu run
   # Should output: x = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
   ```

3. Check PATH:
   ```bash
   # Windows
   where qu
   
   # Should show path to qu.exe
   ```

---

## Quick Fix Checklist

Run these commands in order:

```bash
# 1. Update everything
rustup update
npm install -g npm

# 2. Clean builds
cd .                          # the repository root
cargo clean

cd qu-studio-tauri
rm -rf node_modules
rm package-lock.json

# 3. Install dependencies
npm install

# 4. Build Qu CLI
cd engine/crates/qu-cli
cargo build

# 5. Test Qu CLI
cd ../../target/debug
./qu.exe version

# 6. Run QuStudio
cd ../../qu-studio-tauri
npm run tauri dev
```

---

## System Requirements

### **Minimum:**
- Windows 10 / macOS 10.15 / Linux (Ubuntu 18.04+)
- 4 GB RAM
- 2 GB disk space
- Node.js 18+
- Rust 1.70+

### **Recommended:**
- 8 GB RAM
- SSD storage
- Node.js 20+
- Rust 1.75+

---

## Verify Installation

Run these commands to verify everything is set up correctly:

```bash
# Check Node.js
node --version
# Should be: v18.x.x or higher

# Check npm
npm --version
# Should be: 9.x.x or higher

# Check Rust
rustc --version
# Should be: rustc 1.70.0 or higher

# Check Cargo
cargo --version
# Should be: cargo 1.70.0 or higher

# Check Qu CLI
qu version
# Should be: qu 0.1.0

# Check Tauri CLI
tauri --version
# Should be: tauri-cli 1.5.0 or higher
```

---

## Alternative Launch Methods

### **Method 1: Direct npm (if batch file fails)**
```bash
cd qu-studio-tauri
npm run tauri dev
```

### **Method 2: VS Code Terminal**
1. Open VS Code in the `qu-studio-tauri` folder
2. Open terminal (Ctrl+`)
3. Run: `npm run tauri dev`

### **Method 3: Build & Run Separately**
```bash
# Build
npm run tauri build

# Run the built executable
cd src-tauri/target/release
./QuStudio.exe
```

---

## Getting Help

If you're still stuck:

1. **Check logs:**
   - Terminal output
   - DevTools console (F12)
   - Rust compilation errors

2. **Common fixes:**
   - Clean and rebuild
   - Update dependencies
   - Check PATH variables

3. **Verify prerequisites:**
   - Node.js installed?
   - Rust installed?
   - Qu CLI built?

4. **Still stuck?**
   - Copy the full error message
   - Check what step failed
   - Try the Quick Fix Checklist above

---

## Performance Tips

### **Slow first build?**
- Normal! Rust compilation takes time
- Subsequent builds are much faster (10-20s)

### **App running slow?**
- Close other applications
- Check RAM usage
- Try release build: `npm run tauri build`

### **High memory usage?**
- Normal for development mode
- Release build uses less memory
- Close unused tabs/panels

---

## Success Indicators

You know it's working when:

✅ `qu version` shows version number  
✅ `npm run tauri dev` starts without errors  
✅ App window opens  
✅ Code editor is visible  
✅ Run button is clickable  
✅ Terminal shows "Qu REPL ready"  

---

**Happy coding!** 🚀
