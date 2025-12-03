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
* [ ] ~~silence zerr, zwarning during parsing~~
* [ ] ~~capture zerr, zwarning during completion~~
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
* [ ] heredocs
* [ ] selecting `print -s echo` in history is weird
* [x] silence parse warnings
* [ ] custom buffer rendering, ghost text etc
* [x] fix segfault when letting zle exit by itself
* [ ] ~~try switch to termion~~
* [x] remove extra zle prompt after accept line
* [x] lag when exiting. does it happen in release?
* [ ] calling exit within widget causes hang
* [ ] magic space for completion
* [ ] snippets?
* [ ] job status reporting
* [x] recursive keymaps
* [ ] recursive keymaps in lua
* [ ] fork safety
    * zsh will fork e.g. `( echo 123 )` and we have threads and locks, yikes
    * is this safe? what do we need to deal with? do we need some `pthread_atfork`?
    * [x] is the shell fork safe?
        * it seems to work? `( wsh lua 'wish.cmd[[ ... ]]' )` seems to have no problems
            due to the extra shell lock permit with `wsh lua` to allow recursion
        * are there other ways hit the shell lock?
            all other threads are dead so really you can run zsh (safe) and `wsh lua` and i think thats it
    * [ ] is the ui fork safe?
        * if someone takes the ui lock right before fork, it will get lost and ui will be inaccessible
        * `( wsh lua 'wish.set_buffer("x"); print(wish.get_buffer())' )`
        * you can still control-c it, but not nice
    * [ ] is the ui init fork safe?
        * the lock is used on init and `wsh lua ...`
        * same as above, if someone takes the lock before fork, you get a hang
    * [ ] is lua fork safe?
        * uhhh not really, see hacks in `./src/externs/fork.rs`
    * [ ] is `has_foreground_process` fork safe?
        * i guess we don't care, it needs to remain locked in children anyway
    * [ ] is crossterm fork safe?
        * crossterm is not used for input, nor does the child need terminal input access so that's fine
        * crossterm has a lock around a static term settings variable,
            this can only be accessed via the `UiInner` so it depends on the fork safety of that
    * [ ] is the logger fork safe?
        * there's locks around the output stream
        * on that note i guess the same holds for std stdin, stdout, stderr locks
    * [ ] is tokio fork safe?
        * no idea
        * seems to use tls for the handler, maybe its ok? dunno
* [ ] control c style escape hatch
* [ ] TMOUT
* [ ] scrolling widgets
* [ ] horizontal widget layout
* [ ] highlight colour system
* [ ]
