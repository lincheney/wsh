// c definitions for things that are not in zsh_sys
// esp zle things

/*
 *
 * This file is part of zsh, the Z shell.
 *
 * Copyright (c) 1992-1997 Paul Falstad
 * All rights reserved.
 *
 * Permission is hereby granted, without written agreement and without
 * license or royalty fees, to use, copy, modify, and distribute this
 * software and to distribute modified versions of this software for any
 * purpose, provided that the above copyright notice and the following
 * two paragraphs appear in all copies of this software.
 *
 * In no event shall Paul Falstad or the Zsh Development Group be liable
 * to any party for direct, indirect, special, incidental, or consequential
 * damages arising out of the use of this software and its documentation,
 * even if Paul Falstad and the Zsh Development Group have been advised of
 * the possibility of such damage.
 *
 * Paul Falstad and the Zsh Development Group specifically disclaim any
 * warranties, including, but not limited to, the implied warranties of
 * merchantability and fitness for a particular purpose.  The software
 * provided hereunder is on an "as is" basis, and Paul Falstad and the
 * Zsh Development Group have no obligation to provide maintenance,
 * support, updates, enhancements, or modifications.
 *
 */


#define mod_export static;

#include <stdint.h>
#include <unistd.h>
typedef size_t LinkList;
typedef uint32_t mode_t;

mod_export static unsigned char Meta = ((char) 0x83);
mod_export static unsigned char Inpar = ((char) 0x88);
mod_export static unsigned char Outpar = ((char) 0x8a);

typedef struct cmatcher  *Cmatcher;
typedef struct cmlist    *Cmlist;
typedef struct cpattern  *Cpattern;
typedef struct menuinfo  *Menuinfo;
typedef struct cexpl *Cexpl;
typedef struct cmgroup *Cmgroup;
typedef struct cmatch *Cmatch;

/// <div rustbindgen nocopy></div>
struct cmatch {
    char *str;			/* the match itself */
    char *orig;                 /* the match string unquoted */
    char *ipre;			/* ignored prefix, has to be re-inserted */
    char *ripre;		/* ignored prefix, unquoted */
    char *isuf;			/* ignored suffix */
    char *ppre;			/* the path prefix */
    char *psuf;			/* the path suffix */
    char *prpre;		/* path prefix for opendir */
    char *pre;			/* prefix string from -P */
    char *suf;			/* suffix string from -S */
    char *disp;			/* string to display (compadd -d) */
    char *autoq;		/* closing quote to add automatically */
    int flags;			/* see CMF_* below */
    int *brpl;			/* places where to put the brace prefixes */
    int *brsl;			/* ...and the suffixes */
    char *rems;			/* when to remove the suffix */
    char *remf;			/* shell function to call for suffix-removal */
    int qipl;			/* length of quote-prefix */
    int qisl;			/* length of quote-suffix */
    int rnum;			/* group relative number */
    int gnum;			/* global number */
    mode_t mode;                /* mode field of a stat */
    char modec;                 /* LIST_TYPE-character for mode or nul */
    mode_t fmode;               /* mode field of a stat, following symlink */
    char fmodec;                /* LIST_TYPE-character for fmode or nul */
};

typedef struct cmgroup *Cmgroup;

struct cmgroup {
    char *name;			/* the name of this group */
    Cmgroup prev;		/* previous on the list */
    Cmgroup next;		/* next one in list */
    int flags;			/* see CGF_* below */
    int mcount;			/* number of matches */
    Cmatch *matches;		/* the matches */
    int lcount;			/* number of things to list here */
    int llcount;		/* number of line-displays */
    char **ylist;		/* things to list */
    int ecount;			/* number of explanation string */
    Cexpl *expls;		/* explanation strings */
    int ccount;			/* number of compctls used */
    LinkList lexpls;		/* list of explanation string while building */
    LinkList lmatches;		/* list of matches */
    LinkList lfmatches;		/* list of matches without fignore */
    LinkList lallccs;		/* list of used compctls */
    int num;			/* number of this group */
    int nbrbeg;			/* number of opened braces */
    int nbrend;			/* number of closed braces */
    int new;			/* new matches since last permalloc() */
    /* The following is collected/used during listing. */
    int dcount;			/* number of matches to list in columns */
    int cols;			/* number of columns */
    int lins;			/* number of lines */
    int width;			/* column width */
    int *widths;		/* column widths for listpacked */
    int totl;			/* total length */
    int shortest;		/* length of shortest match */
    Cmgroup perm;		/* perm. alloced version of this group */
#ifdef ZSH_HEAP_DEBUG
    Heapid heap_id;
#endif
};

mod_export LinkList matches;
mod_export Cmgroup lastmatches, pmatches, amatches, lmatches, lastlmatches;
mod_export char **cfargs;
mod_export int cfret;
mod_export char *compfunc = NULL;
mod_export int nbrbeg, nbrend;

mod_export int
menucomplete(char **args);

mod_export void
makezleparams(int ro);

mod_export int
permmatches(int last);

mod_export void
do_single(Cmatch m);

mod_export void
metafy_line(void);
mod_export void
unmetafy_line(void);

struct menuinfo {
    Cmgroup group;		/* position in the group list */
    Cmatch *cur;		/* match currently inserted */
    int pos;			/* begin on line */
    int len;			/* length of inserted string */
    int end;			/* end on the line */
    int we;			/* non-zero if the cursor was at the end */
    int insc;			/* length of suffix inserted */
    int asked;			/* we asked if the list should be shown */
    char *prebr;		/* prefix before a brace, if any */
    char *postbr;		/* suffix after a brace */
};
mod_export struct menuinfo minfo;

int expandhistory();

void set_histno(void* pm, long x);
int selectkeymap(char *name, int fb);
void initundo(void);

int acceptline();
