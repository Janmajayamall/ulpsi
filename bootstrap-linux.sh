#!/bin/bash

# install protoc
echo "Installing protoc compiler..."
PB_REL="https://github.com/protocolbuffers/protobuf/releases"
PKG="protoc-23.4-linux-x86_64.zip"

curl -LO $PB_REL/download/v23.4/$PKG

unzip $PKG -d $HOME/.local

export PATH="$PATH:$HOME/.local/bin"

rm $PKG
