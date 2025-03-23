# NextUI Updater PAK

A PAK for updating NextUI on-device, tested on the TrimUI Brick (but might work on the Smart Pro as well). Wifi required (obviously).

Mainly created as a learning experience for me, but despite its simplicity I think it might be useful for others.

## Installation

Unzip the `nextui-updater-pak.zip` file to the root of your SD card (merge the contents).

## Controls

The application supports both keyboard and game controller input:

- **D-pad Up/Down**: Navigate between buttons
- **Button A**: Select

## Building for tg5040 using [cross-rs](https://github.com/cross-rs/cross)

```bash
cross build --release --target=aarch64-unknown-linux-gnu
```

The compiled binary will be in `target/aarch64-unknown-linux-gnu/release/nextui-updater-rs`.

## Building release zip

```bash
scripts/create_pak.sh
```

The zip file will be in `./nextui-updater-pak.zip`.

## License

This project is open source and available under the MIT License.

## Contributing

Many improvements are possible and contributions are welcome! Please feel free to submit a Pull Request.
