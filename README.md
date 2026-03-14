# G-lang

A dynamically-typed interpreted programming language written in Rust.

G-lang is a lightweight, expressive language designed for simplicity and ease of use. It provides a clean syntax with support for modern programming paradigms including functions, structs, and a flexible type system.

## Features

- **Dynamically-typed**: No need for explicit type annotations
- **Functions**: First-class functions with closures
- **Structs**: Define custom data structures with fields and methods
- **Control Flow**: if/else conditions, while loops, for loops, and break/continue statements
- **Built-in Types**: Integers, bigIntegers, booleans, strings, arrays, and hash maps
- **Module System**: Import and organize code across multiple files
- **REPL Mode**: Interactive Read-Eval-Print Loop for quick experimentation
- **Standard Library**: Built-in functions for common operations
- **Async and Await**: Support for asynchronous operations
- **Wasm runtime**: Allows importing and running functions from .wasm and .wat files

## Installation

### Prerequisites

- Rust 1.70 or later ([install Rust](https://www.rust-lang.org/tools/install))

### Build from Source

1. Clone the repository:
```bash
git clone https://github.com/Lucop1911/g-lang.git
cd g-lang
```

2. Build the project:
```bash
cargo build --release
```

3. The executable will be located at `target/release/g-lang`

### Add to PATH (Optional)

To run `g-lang` from anywhere:

**On Linux**
```bash
export PATH="$PATH:$(pwd)/target/release"
```

**On Windows:**
Add `C:\path\to\g-lang\target\release` to your system PATH environment variable.

## Quick Start

### REPL Mode

Start the interactive REPL:
```bash
g-lang
```

### Run a Script

Execute a `.g` file:
```bash
g-lang run script.g
```

### Basic Examples

**Variables and Arithmetic:**
```
let x = 10;
let y = 20;
let sum = x + y;
println(sum);
```

**Functions:**
```
fn add(a, b) { a + b };
println(add(5, 3));
```

**Arrays:**
```
let arr = [1, 2, 3, 4, 5];
println(len(arr));
println(head(arr));
```

**Control Flow:**
```
let x = input("insert a number: ").to_int();
if (x > 10) {
    println("x is greater than 10");
} else {
    println("x is less than or equal to 10");
}
```

**While Loop:**
```
let counter = 0;
while (counter < 5) {
    println("Counter: ", counter);
    counter = counter + 1;
}
```

**For Loop (Iterator-style):**
```
let fruits = ["apple", "banana", "cherry"];
for (fruit in fruits) {
    println("I like ", fruit);
}
```

**For Loop (C-style):**
```
for (let i = 0; i < 10; i = i + 1) {
    if (i % 2 == 0) {
        println(i, " is even");
    }
}
```

**Structs:**
```
struct Person {
    name: null,
    age: null,
    
    greet: fn() {
        println("Hello, I'm ", this.name);
    }
}

let person = Person { name: "John", age: 30 };
person.greet();
```

## Usage

```
USAGE:
    g-lang [COMMAND] [OPTIONS]

COMMANDS:
    (no command)       Start the REPL (Read-Eval-Print Loop)
    run <file>         Execute a .g file
    check <file>       Parse and check a .g file for errors

OPTIONS:
    -h, --help         Print this help message
    -v, --version      Print version information
```

## Documentation

For comprehensive documentation and more examples, please visit the [official documentation website](https://g-language.vercel.app)

## License

This project is licensed under the Apache License 2.0 - see the [LICENSE](LICENSE) file for details.

## Repository

- **GitHub**: [https://github.com/Lucop1911/g-lang](https://github.com/Lucop1911/g-lang)