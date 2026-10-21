#ifndef FM_EXPLORE_H
#define FM_EXPLORE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct FmClient FmClient;

/*
 * Progress callback. `total` is -1 when the total size is unknown.
 * The callback runs on the thread that executes fm_call().
 */
typedef void (*FmProgressCallback)(void *user, uint64_t current, int64_t total);

FmClient *fm_client_new(uint16_t vid, uint16_t pid, uint32_t timeout_ms,
                        uint32_t retries, uint32_t maker_wait_ms, int verbose);
void fm_client_free(FmClient *client);
void fm_client_close(FmClient *client);
void fm_client_set_progress_callback(FmClient *client,
                                     FmProgressCallback callback, void *user);

/*
 * Executes a command and returns a JSON object owned by the caller:
 *   {"ok":true,"data":...}
 *   {"ok":false,"error":"...","kind":"...","status":null,"os_error":null}
 * Free the returned string with fm_free_string().
 *
 * Commands and arguments:
 *   connect
 *   disconnect
 *   list_partitions
 *   list_directory       arg1=remote path
 *   list_entries         arg1=remote path (listing enriched with file info)
 *   list_file            arg1=remote path
 *   mkdir                arg1=remote path
 *   delete               arg1=remote path
 *   set_readonly         arg1=remote path, arg2="on"|"off"
 *   checkdavailable
 *   pull                 arg1=remote path, arg2=local path
 *   push                 arg1=local path,  arg2=remote path
 *   version
 */
char *fm_call(FmClient *client, const char *command,
              const char *arg1, const char *arg2);
void fm_free_string(char *value);
const char *fm_version(void);

#ifdef __cplusplus
}
#endif

#endif /* FM_EXPLORE_H */
