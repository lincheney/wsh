# wsh

# wish-list

i wish i could have
* easy async
* easy async prompt
* easy status bar
* easy whatever widgets i want
* easy live preview
* redirect/repipe output live
* async completion
* pluggable and customisable completion
* completion with better syntax awareness
* nicer detection of if the previous command doesn't end with newline
* plugin system in lua? lots of stuff probably too hard to configure in bash
* bg/async jobs with nicely interleaved output
* load history in parallel?
    big histfiles make shell start up slow,
    but it seems a shame to lose history just to make the shell faster
    maybe should just use something like atuin instead?
* nice nvim integration
    * with treesitter highlighting
* easy hooks, on keystroke, on whatever

# TODO

* [x] buffer edit history, undo, redo
* [x] cut and paste
* [ ] up/down work in multiline editing?
* [ ] <alt-.>, insert-last-word
* [x] edit-command-line, i.e. in vim
* [ ] control over zerr, zwarning
* [ ] silence zerr, zwarning during parsing
* [ ] capture zerr, zwarning during completion
* [ ] drop history entries which are space etc
* [x] general selection widget interface
* [ ] embed process output in a tui message
* [ ] draw prompt and buffer using ratatui
* [ ] status bar
* [ ] poc async prompt
* [x] call lua from zsh
* [ ] fork and run zsh
* [ ] var for last term cursor position
* [ ] options system
* [x] alias, history expansion
* [ ] heredocs
* [ ] 
