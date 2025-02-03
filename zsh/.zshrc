module_path+=( "$ZDOTDIR" )
>&2 printf "DEBUG(puff)  \t%s\n" "$(declare -p module_path)"
zmodload wish_debug
wash
zmodload -F -l wish_debug
