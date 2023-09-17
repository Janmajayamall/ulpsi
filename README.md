# Unbalanced labelled (private) set intersection (ulpsi)

The repository implements labelled unbalanced set intersection where the client's set stays private and server's set is public and client's set size is way smaller than server's set size. The server returns the labels corresponding to item in intersection. We use the technique implemented in https://github.com/microsoft/APSI without privacy of server's set.

For now query parameters are fixed. Both item and its corresponding label should be of size 256 bits and client's set may contain upto 4000 items. Server's set can be arbitrarily large.

The implementation is not optimised for memory nor for performance and was only intended to test the client-server communication cost. If either memory and performance seem to be bottleneck, they can be improved upon.

Checkout [notes](./notes/Labelled%20PSI.md) for implementation details.

## Run

Note: Protoc compiler >= 23.4 is required. You can install it from [here](https://grpc.io/docs/protoc-installation/). Alternatively, if you are on linux then try running the following [script](./bootstrap-linux.sh).

First `cd` into `./server` and run setup with desired server's set size.

For example, we run setup with 1M server set size.

```
export MIL=1000000
cargo run --release -- setup $MIL
```

Depending on the set size, setup might anywhere between a few minutes to an hour.

After setting up the server, randomly generate client set. For example, with server set size set to 1000000, to randomly generate client set of size 4000 run the following:

```
cargo run --release -- gen-client-set $MIL 4000
```

This stores the client_set.bin file under `./../data/1000000`

Finally, start the server. For example, if you ran setup for 1M then run the following:

```
cargo run --release -- setup $MIL
```

To test whether server returns corresponding labels to items in random client set generated using `gen-client-set`, change to `client` directory. Then run

```
cargo run --release -- ./path/to/client_set.bin
```

For example, if you ran `gen-client-set` for server set size 1M and client set 4000 then set the path to `./../data/1000000/client_set.bin`.

## Benchmarks

| Machine          | Cient set size | Server set size | Item size (bits) | Label size (bits) | Client upload cost (MB) | Client download cost (MB) | Server runtime (ms) |
| ---------------- | -------------- | --------------- | ---------------- | ----------------- | ----------------------- | ------------------------- | ------------------- |
| [m6a.16xlarge]() | 4000           | 16M             | 256              | 256               | 21                      | 14.1                      | 3728                |

## To do's

1. Reduce run-time memory by storing `item_data` and `label_data` of `InnerBox` as buffers instead of `Array2<u32>`.
2. Replace prints with proper logging.
3. Enable updating server's set at run-time.
4. Add mroe benchmarks with different set sizes.
