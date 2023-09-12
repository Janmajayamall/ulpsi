#!/bin/bash

git checkout dev

BILLION=1000

# server setup for 1 billion
cd ./server
cargo run --release -- setup $BILLION

# Client set of size 4000 with server set 1 billion
cargo run --release -- gen-client-set $BILLION 500

# Start server 
cargo run --release -- start $BILLION
