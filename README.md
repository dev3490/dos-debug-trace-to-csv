# dos-debug-trace-to-csv 
I'm a student who is learning TASM on DOSBox-X (8086 CPU), and I have to write a report with a trace table for every program.

It was all fun and games until I found out that neither Turbo Debugger nor the `debug` console program from DOSBox-X (actually FreeDOS) can export a trace in any popular format, so I had to write trace tables manually.

As a programmer, I couldn't accept this fate, so I wrote this program, which parses the output of that `debug` program and converts it to CSV, which I can later insert into my Typst report.

---

It's not quite that simple, though. There are a few limitations:
1. `debug` cannot show the actual source code; it shows disassembly, which I later need to replace with source code
2. `debug` starts with ZF and PF set (ZR, PE), while `td` starts with only IF set, so the flags have to be reset manually through `debug` options
3. `debug` program by default sets the CX register to the program size
4. Programs are loaded at different memory addresses under `debug` and `td`, so segment values differ
5. Programs that take input from the keyboard cannot be traced

But anyway this program has simplified my life: it fills in 90% of the trace table for me.

## Usage
```bash
dt2c <DEBUG PROGRAM OUTPUT FILE>
```

The program will create `<DEBUG PROGRAM OUTPUT FILE WITHOUT EXTENSION>.csv` file in the current working directory

### Running `debug` properly
To work around limitations 2 and 3:
1. Create a file named `debug.txt` with the following content. The file must be accessible inside DOS:
```
m ds:1a L1 ds:19
r f
NZ PO
r cx
0
r
t 200
q
```

- `t 200` traces up to 0x200 (512) instructions
- `m ds:1a L1 ds:19` redirects the traced program's standard output to the screen, so it doesn't end up in the trace file
- `r` prints the initial state, which becomes the first row of the table

2. Run `debug` inside DOS
```bat
debug your-prog.exe < debug.txt > trace.txt
```

### Running the program
```bash
dt2c trace.txt
```

The CSV column set matches the trace table format required by my course, but it can be easily changed by writing a new struct that implements `From<&TraceStep<'s>>`

## Installation
```bash
git clone <future repo> && cd <future repo>
cargo install --path .
```

## License
Licensed under GPL-3.0-or-later. You can find the full license text in the `LICENSE` file
