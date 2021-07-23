# :cloud: Rust Cloud Readers :cloud:

[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

This library contains data structures and functions to help read data from Cloud storages (or more generally, from the network).

## Context

Most client library to read from the network in Rust are async. One example is the AWS S3 client library. On the other hand, data processing systems prefer to read data using the `std::io::Read` trait that is blocking, partly because it is more performant for large files, partly beacause many data processing libraries such as protobof use that trait in their interfaces. In that case, the data processing flow looks like this:

![General flow generic](https://raw.githubusercontent.com/wiki/cloudfuse-io/cloud-readers-rs/general_flow_generic.jpg)

If we apply this to the example of [Buzz](https://github.com/cloudfuse-io/buzz-rust), we get the following flow:

![General flow buzz](https://raw.githubusercontent.com/wiki/cloudfuse-io/cloud-readers-rs/general_flow_buzz.jpg)

Additionally, the strategy to fetch data from the network will be different from reading from a high bandwith hard drive. For local data, the bottleneck might be the processing resource and not the disk bandwidth, and even if that is not the case, you will usually not get much benefit from reading multiple chunks in parallel as one read stream will already fully utilizes the bandwidth of the disk (this might not be completely true when multiple drives are attached). That is completely different when reading from the network where each read will usually be throttled on the data provider side. On AWS S3 for instance, you get a much better overall bandwidth if you read 8 chunks in parallel (even for a single file). This means that it is worth decoupling the read strategy from the processing by eagerly downloading and caching in memory for future processings.

## Proposed solution

The proposed solution is to introduce a dedicated datastructure that takes care of scheduling the downloads and caching the results, while providing a blocking API that implements the `std::io::Read` trait. It scheduling strategy is customizable to adapt to different types of reads (chunks of a Parquet file will be read in a different order thant those of a CSV file) and different infrastructures (Cloud storage will behave differently from an on-premise Hadoop cluster). But most the caching and synchronization mechanismes will be common to most use cases.

The proposed strategy is the following one:

![Solution flow generic](https://raw.githubusercontent.com/wiki/cloudfuse-io/cloud-readers-rs/solution_flow_generic.jpg)

If we apply it to the specific usecase of [Buzz](https://github.com/cloudfuse-io/buzz-rust), it boils down to this:

![Solution flow buzz](https://raw.githubusercontent.com/wiki/cloudfuse-io/cloud-readers-rs/solution_flow_buzz.jpg)