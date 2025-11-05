# Introduction

Welcome to **The Pilatus Book**!

Pilatus is a modular, extendable application framework designed for industrial and computer vision projects. While the core framework is not limited to engineering applications, it is particularly well-suited for projects with autonomous configurable subsystems that interact with one another.

## What is Pilatus?

Pilatus provides a robust foundation for building complex, asynchronous applications with the following key features:

- **Asynchronous Devices**: Run asynchronous actors that communicate via message passing
- **Recipe Management**: Switch between different sets of device configurations without restarting the application
- **Extensibility**: Add new extensions without requiring changes to the core framework
- **API Integration**: Easily expose core functionality through various APIs (HTTP, WebSockets, MQTT etc.)
- **Cross-Platform**: Support for multiple platforms (Linux, macOS, Windows) and architectures (x64, ARM)

## Who is this book for?

This book is intended for developers who want to:

- Build industrial automation or computer vision applications
- Avoid wasting time to write boilderplate code
- Use existing code for common tasks and easily create reusable internal extensions
- Build upon existing code for common tasks and easily package them as reusable internal extensions

Many developers find that proprietary frameworks from individual companies fail to meet their specific requirements and result in vendor lock-in. Pilatus addresses these concerns by being open source under the MPL2 license, which permits proprietary usage with your custom proprietary extensions. The framework provides a growing number of flexible tools that may align with your needs. When they do not, only a small set of flexible core concepts are required by default. Official extensions, such as the Axum webserver, are built on the same APIs that are available to your own extensions.

## How to use this book

This book assumes you have a basic understanding of:

- Rust programming language
- Asynchronous programming concepts

If you're new to Rust, we recommend working through [The Rust Programming Language Book](https://doc.rust-lang.org/book/) first.


Let's get started!

