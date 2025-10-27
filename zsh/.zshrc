module_path+=( "$ZDOTDIR" )
>&2 printf "DEBUG(puff)  \t%s\n" "$(declare -p module_path)"
# HISTFILE=/tmp/zsh.history

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

PROMPT=$'%K{10}%F{0}hello 林\n>>>%k%f '
PROMPT=$'%F{10}>>> %f'

# _main_complete() {
    # sleep 5
    # compadd x
# }
eval "_aws(){ $(cat /home/qianli/setup/vendor/aws-cli-completion.git/zsh/_aws); }; compdef _aws aws"
# eval "_aws(){ compadd x; }; compdef _aws aws"


zle-line-init(){
    builtin wsh
}
zle -N zle-line-init

# _main_complete() {
    # echo 123 >/dev/tty
# }
# zle -C _main_complete complete-word _main_complete
