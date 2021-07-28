# syntax=docker/dockerfile:experimental

# This docker file contains different build targets of the application
# Each target is represented by a stage

## BUILD STAGE ##
# Create a static binary (with musl) for one of the binary targets.

FROM rust:1.53.0-buster as build-stage
ARG BIN_NAME

# install environment

RUN apt-get update
RUN apt-get install musl-tools zip -y

WORKDIR /buildspace

RUN rustup target add x86_64-unknown-linux-musl && rustup component add rustfmt 

COPY Cargo.lock .
COPY Cargo.toml .
COPY ../src/ src/ 

# use BuildKit experimental cache mount to speed up builds
RUN --mount=type=cache,target=./target \
  --mount=type=cache,target=/usr/local/cargo/git \
  --mount=type=cache,target=/usr/local/cargo/registry \
  cargo build --bin ${BIN_NAME} --release --target=x86_64-unknown-linux-musl && \
  cp ./target/x86_64-unknown-linux-musl/release/${BIN_NAME} ./exec-static

## LAMBDA PACKAGE STAGE ##
# Create a zip archive to be deployed to AWS Lambda

FROM build-stage as package-stage
ARG BIN_NAME
# the exec name inside a lambda archive should be `bootstrap`
RUN cp ./exec-static ./bootstrap
RUN zip ${BIN_NAME}.zip bootstrap

## INFRA STAGE ##
# Import the package into a terraform container to deploy it

FROM hashicorp/terraform as terraform
ARG BIN_NAME
WORKDIR /infra
COPY --from=package-stage /buildspace/${BIN_NAME}.zip .
COPY notebooks/lambda.tf .