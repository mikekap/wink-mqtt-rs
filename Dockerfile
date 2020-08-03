FROM ubuntu:20.04

WORKDIR /tmp/

RUN apt update && apt install -y build-essential curl wget
RUN wget https://musl.cc/arm-linux-musleabi-cross.tgz && tar -zxvf arm-linux-musleabi-cross.tgz --strip-components 1 -C /usr/local/
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"
RUN rustup target add armv5te-unknown-linux-musleabi
