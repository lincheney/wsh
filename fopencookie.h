typedef struct FILE FILE;
typedef struct ssize_t ssize_t;
typedef struct size_t size_t;

extern FILE *stdin;		/* Standard input stream.  */
extern FILE *stdout;		/* Standard output stream.  */
extern FILE *stderr;		/* Standard error output stream.  */

typedef ssize_t cookie_read_function_t (void *__cookie, char *__buf,
                                          size_t __nbytes);
typedef ssize_t cookie_write_function_t (void *__cookie, const char *__buf,
                                          size_t __nbytes);
typedef int cookie_seek_function_t (void *__cookie, ssize_t *__pos, int __w);
typedef int cookie_close_function_t (void *__cookie);

typedef struct _IO_cookie_io_functions_t
{
  cookie_read_function_t *read;		/* Read bytes.  */
  cookie_write_function_t *write;	/* Write bytes.  */
  cookie_seek_function_t *seek;		/* Seek/tell file position.  */
  cookie_close_function_t *close;	/* Close file.  */
} cookie_io_functions_t;

extern FILE *fopencookie (void *__restrict __magic_cookie,
			  const char *__restrict __modes,
			  cookie_io_functions_t __io_funcs);

