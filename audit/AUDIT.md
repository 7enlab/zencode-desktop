# AUDIT - npm cowsay@1.6.0 (MIT) · project: ZenCode Desktop (pilot)
Source read: lib/balloon.js, index.js, cli.js, cows/default.cow.
- Input: CLI args joined by " "; no args -> stdin, trailing newline stripped.
- Normalize: \r\n and \r -> \n; \t -> 8 spaces.
- Wrap (default on, -W 40): per input line, HARD cut every W UTF-16 code units (not words). -n disables.
- Empty text -> zero lines -> " __" / " --" (top/bottom of width 0+2).
- Width/padding: string-width (East Asian wide/fullwidth = 2, combining = 0, ANSI stripped).
- One line: "< text >". Multi: "/ \", "| |", "\ /", each padded to max width.
- Top " " + "_"*(max+2); bottom " " + "-"*(max+2). Then default cow, thoughts "\".
Golden cases: spec/cases/NN.json (argv array), optional NN.stdin, NN.out (exact stdout).
