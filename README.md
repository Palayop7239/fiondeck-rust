<h1 align="center">
  <br>
  <!-- 
  <a>
    <img src="assets/fiondeck.png" alt="might do a logo" width="200">
  </a>
  -->
  <br>
  <b>Fiondeck</b>
  <br>
</h1>

<h4 align="center"> A custom app for controlling Loupedeck devices written in Rust
</h4>

<p align="center">
  <a href="#planned-features">Planned Features</a> |
  <a href="#faq">FAQ</a> |
  <a href="#building">Building</a>
</p>

> [!IMPORTANT]
> Fiondeck is early in development. I'm a Rust beginner so this serves me as a learning project, don't expect high quality code.
> <br>
> I used AI as a tool to help me learning and improving some code parts. I prefer being transparent with this
## Credits
Thanks to 
- **foxxyz** for the research on [foxxyz/loupedeck](https://github.com/foxxyz/loupedeck)


## Planned features
  - Supported devices (testing for other devices would be welcome!)
    - [x] Loupedeck Live S (Testing done on the Live S)
  - [x] Configuration & Bindings (See bindings.toml)
  - [ ] Plugins API for Lua plugins (Custom Buttons & more)
  - [ ] GIF Support
  - [ ] Fully usable GUI app
  
**Other features are going to get added here too!**

## FAQ
- **Device is already used?**
  Don't forget to fully shutdown the official app!

**Other questions will be added here later on!**

## Building
If you want to compile the code by yourself, it's very simple.
<br>

**1. Download Rust & Cargo if you don't already have it**
https://rust-lang.org/tools/install/

<br>

**2. Download the source code or use Git to clone it**
<br>

```sh
git clone https://github.com/Palayop7239/fiondeck-rust.git
```
<br>

**3. Compile Fiondeck using Cargo**


```sh
cd fiondeck-rust
cargo run
```
<br>