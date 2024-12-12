#!/usr/bin/env bash
set -o nounset -e

RUSTFLAGS="-C relocation-model=static" cargo build --release --target x86_64-unknown-none
mkdir -p boot_root/EFI/BOOT
cp BOOTX64.EFI boot_root/EFI/BOOT/
cp target/x86_64-unknown-none/release/credits boot_root/
cp limine.conf boot_root/
# this is from nixos wiki
qemu-system-x86_64-uefi -enable-kvm -drive format=raw,file=fat:rw:boot_root -serial stdio -m 1G
