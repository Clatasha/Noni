<p align="center">
  <img src="packaging/icons/noni.png" alt="Noni fruit logo" width="168">
</p>

<h1 align="center">Noni Video Editor</h1>

Noni is Clatasha's personal video editor for Windows and Linux. It has a track-based timeline, clip effects and animations, and a dedicated export dialog.

New projects are saved as `.noniproj`. Noni can also open `.bvproj` projects.

## Build from source

Use Rust 1.95 or newer. See [the build instructions](CONTRIBUTING.md) for platform dependencies.

```bash
git clone --branch noni-branding https://github.com/Clatasha/Noni.git
cd Noni
cargo run --release -p bananav
```

Noni is based on [BananaV](https://github.com/acasoq-web/BananaV). See [LICENSE](LICENSE) and [ATTRIBUTION.md](ATTRIBUTION.md) for license and contributor information.
