# Unbalanced labelled (private) set intersection (ulpsi)

This repository implements "unbalanced labelled private set intersection" where client's set stays private and server's set is public and client's set is way smaller than server's set. Moreover, instead of returning boolean flag indicating items at intersection, server returns labels corresponding to items at intersection. Implementation is based on protocol introduced in https://github.com/microsoft/APSI without privacy of server's set.

For now query parameters are fixed. Both item and its corresponding label should be of size 256 bits and client's set may contain upto 4096 items. Server's set can be arbitrarily large.

The implementation is not optimised for memory nor for performance and was only intended to test the client-server communication cost. If either memory and performance seem to be bottleneck, they can be improved upon.

Checkout [notes](./notes/Labelled%20PSI.md) for implementation details.

## Run

> **Note**
> Protoc compiler >= 23.4 is required. You can install it from [here](https://grpc.io/docs/protoc-installation/). Alternatively, if you are on linux then try running the following [script](./bootstrap-linux.sh).

First `cd` into `./server` and run setup with desired server's set size.

For example, we run setup with 1M server set size.

```
export MIL=1000000
cargo run --release -- setup $MIL
```

Depending on the set size, setup might take anywhere between a few minutes to an hour.

After setting up the server, randomly generate client set. For example, with server set size set to 1000000, to randomly generate client set of size 4000 run the following:

```
cargo run --release -- gen-client-set $MIL 4000
```

This stores the client_set.bin file under `./../data/1000000`

Finally, start the server. For example, if you ran setup for 1M then run the following:

```
cargo run --release -- setup $MIL
```

To test whether server returns corresponding labels to items in client set randomly generated above, switch to `client` directory. Then run

```
cargo run --release -- ./path/to/client_set.bin
```

If you ran `gen-client-set` for server set size 1M and client set 4000, as above, then set the path to `./../data/1000000/client_set.bin`.

> **Note**
> By default client set size defaults to max. capacity 4096. This is because other parameters are somewhat optimal when client set size is set to 4096. You may choose to decrease max. capacity of client set size by setting `ht_size` in `PsiParams::default` to some power of 2 >= 512. However, I should note that although this should reduce client-server and server-client communication cost, the costs will not be optimal. Most certainly the cost for smaller client set sizes can be reduced by brute forcing and finding optimal parameters.

## Benchmarks

| Machine                                                      | Client set size | Server set size | Item size (bits) | Label size (bits) | Client upload cost (MB) | Client download cost (MB) | Server runtime (ms) |
| ------------------------------------------------------------ | -------------- | --------------- | ---------------- | ----------------- | ----------------------- | ------------------------- | ------------------- |
| [x2idn.16xlarge](https://aws.amazon.com/ec2/instance-types/) | 512            | 10M             | 256              | 256               | 2.55                    | 5.27                      | 2566                |
| [x2idn.16xlarge](https://aws.amazon.com/ec2/instance-types/) | 512            | 100M            | 256              | 256               | 2.55                    | 44.83                     | 16873               |
| [x2idn.16xlarge](https://aws.amazon.com/ec2/instance-types/) | 512            | 300M            | 256              | 256               | 2.55                    | 132.74                    | 48922               |

| Machine                                                      | Client set size | Server set size | Item size (bits) | Label size (bits) | Client upload cost (MB) | Client download cost (MB) | Server runtime (ms) |
| ------------------------------------------------------------ | -------------- | --------------- | ---------------- | ----------------- | ----------------------- | ------------------------- | ------------------- |
| [x2idn.16xlarge](https://aws.amazon.com/ec2/instance-types/) | 1024           | 10M             | 256              | 256               | 5.10                    | 6.05                      | 2897                |
| [x2idn.16xlarge](https://aws.amazon.com/ec2/instance-types/) | 1024           | 100M            | 256              | 256               | 5.10                    | 45.71                     | 17450               |
| [x2idn.16xlarge](https://aws.amazon.com/ec2/instance-types/) | 1024           | 300M            | 256              | 256               | 5.10                    | 133.62                    | 49256               |

| Machine                                                      | Client set size | Server set size | Item size (bits) | Label size (bits) | Client upload cost (MB) | Client download cost (MB) | Server runtime (ms) |
| ------------------------------------------------------------ | -------------- | --------------- | ---------------- | ----------------- | ----------------------- | ------------------------- | ------------------- |
| [x2idn.16xlarge](https://aws.amazon.com/ec2/instance-types/) | 2048           | 10M             | 256              | 256               | 10.19                   | 8.20                      | 3764                |
| [x2idn.16xlarge](https://aws.amazon.com/ec2/instance-types/) | 2048           | 100M            | 256              | 256               | 10.19                   | 47.27                     | 17717               |
| [x2idn.16xlarge](https://aws.amazon.com/ec2/instance-types/) | 2048           | 100M            | 256              | 256               | 10.19                   | 135.28                    | 49617               |

| Machine                                                      | Client set size | Server set size | Item size (bits) | Label size (bits) | Client upload cost (MB) | Client download cost (MB) | Server runtime (ms) |
| ------------------------------------------------------------ | -------------- | --------------- | ---------------- | ----------------- | ----------------------- | ------------------------- | ------------------- |
| [x2idn.16xlarge](https://aws.amazon.com/ec2/instance-types/) | 4000           | 10M             | 256              | 256               | 21                      | 11.6                      | 4798                |
| [x2idn.16xlarge](https://aws.amazon.com/ec2/instance-types/) | 4000           | 16M             | 256              | 256               | 21                      | 14.1                      | 5906                |
| [x2idn.16xlarge](https://aws.amazon.com/ec2/instance-types/) | 4000           | 100M            | 256              | 256               | 21                      | 51.2                      | 18881               |
| [x2idn.16xlarge](https://aws.amazon.com/ec2/instance-types/) | 4000           | 200M            | 256              | 256               | 21                      | 94.6                      | 33976               |
| [x2idn.16xlarge](https://aws.amazon.com/ec2/instance-types/) | 4000           | 300M            | 256              | 256               | 21                      | 138.512                   | 49806               |
| [x2idn.16xlarge](https://aws.amazon.com/ec2/instance-types/) | 4000           | 500M            | 256              | 256               | 21                      | 227                       | 80676               |

> **Note**
> Notice the oddity that only for server set size 10M, client download cost and server runtime gets worse as client set size increases. This is because, for benchmarks, we have re-used optimal parameters for client set size 4096 across all client set sizes.

## To do's

1. Reduce run-time memory by storing `item_data` and `label_data` of `InnerBox` as buffers instead of `Array2<u32>`.
2. Replace prints logging.
3. Enable updating server's set at run-time.
