#!/bin/bash

git checkout dev

BILLION=10000

# server setup for 1 billion
cd ./server
cargo run --release -- setup $BILLION

# Client set of size 4000 with server set 1 billion
cargo run --release -- gen-client-set $BILLION 4000

# Start server 
cargo run --release -- start $BILLION
