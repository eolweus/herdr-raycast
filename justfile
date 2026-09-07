build:
    cargo build --release
    mkdir -p bin
    cp target/release/herdr-raycast bin/herdr-raycast.tmp
    mv bin/herdr-raycast.tmp bin/herdr-raycast

link: build
    herdr plugin link "$PWD"

test:
    cargo test

check:
    cargo clippy --all-targets
