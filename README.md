# wsh

This is super experimental.
I *don't* use this a daily driver.
Expect segfaults.

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
* [x] <alt-.>, insert-last-word
* [x] edit-command-line, i.e. in vim
* [ ] ~~control over zerr, zwarning~~
* [ ] silence zerr, zwarning during parsing
* [ ] capture zerr, zwarning during completion
* [x] drop history entries which are space etc
* [x] general selection widget interface
* [x] embed process output in a tui message
* [x] draw prompt and buffer using ratatui
* [x] status bar
* [x] poc async prompt
* [x] call lua from zsh
* [x] fork and run zsh
* [ ] var for last term cursor position
* [ ] options system
* [x] alias, history expansion
* [x] complete command detection does not work with heredocs
* [ ] selecting `print -s echo` in history is weird
* [x] silence parse warnings
* [x] custom buffer rendering, ghost text etc
* [x] buffer text conceal
* [x] merged prompt and buffer
* [x] $POSTDISPLAY, $PREDISPLAY
* [ ] $region_highlight
* [x] autosuggestions
* [x] fix segfault when letting zle exit by itself
* [ ] ~~try switch to termion~~
* [x] remove extra zle prompt after accept line
* [x] lag when exiting. does it happen in release?
* [ ] calling exit within widget causes hang
* [x] magic space for completion
* [x] file ls colour for completion
* [ ] snippets?
* [ ] capture job status reporting
* [x] recursive keymaps
* [x] recursive keymaps in lua
* [ ] control c style escape hatch
    * [x] lua
    * [ ] anywhere else?
* [ ] TMOUT
* [x] scrolling widgets
* [x] horizontal widget layout
* [ ] highlight colour system
* [x] make zle undo work
* [x] make zle history work
* [ ] vi mode
* [x] can we make `zle -F` work
* [ ] tmux widget backend
* [x] terminal resize
* [x] exit causes shell.func().await to panic
* [x] what exactly needs to be metafied?
* [x] try recover from panic
* [ ]

# Interesting

* https://terminal.click/
* https://codeberg.org/letoram/cat9
