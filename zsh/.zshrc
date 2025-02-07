module_path+=( "$ZDOTDIR" )
>&2 printf "DEBUG(puff)  \t%s\n" "$(declare -p module_path)"
zmodload zsh/complete
# zmodload zsh/zle
zmodload zsh/compctl
autoload -Uz compinit
compinit -C
zmodload wish_debug
# zmodload -F -l wish_debug
_dsv() { source ~/.local/share/zsh/site-functions/_dsv }
compdef _dsv dsv
# compdef _dsv ls
# functions[_main_complete]="set -x; ${functions[_main_complete]}"
zstyle ':completion:*' group-name ''
zstyle ':completion:*' format 'Completing %d'
bindkey '^I' menu-complete
# compdef ls _dsv

_main_complete() {
    sleep 5
    compadd x
}

wash

# _main_complete() {
    # echo 123 >/dev/tty
# }
# zle -C _main_complete complete-word _main_complete
