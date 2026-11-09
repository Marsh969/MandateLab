// LD_PRELOAD shim for constrained containers ONLY (e.g. CI/cloud sandboxes
// whose hard RLIMIT_NOFILE is below 65535). neard refuses to start unless it
// can raise the open-file limit to 65535; this clamps the request to the
// existing hard limit instead of failing. It changes nothing else and is not
// needed on a normal developer machine. Never used for testnet/mainnet nodes.
#define _GNU_SOURCE
#include <dlfcn.h>
#include <sys/resource.h>

typedef int (*setrlimit_fn)(__rlimit_resource_t, const struct rlimit *);

static int clamp_and_set(const char *sym, __rlimit_resource_t res, const struct rlimit *rl) {
    setrlimit_fn real = (setrlimit_fn)dlsym(RTLD_NEXT, sym);
    if (res == RLIMIT_NOFILE && rl) {
        struct rlimit cur;
        if (getrlimit(RLIMIT_NOFILE, &cur) == 0) {
            struct rlimit want = *rl;
            if (want.rlim_max > cur.rlim_max) want.rlim_max = cur.rlim_max;
            if (want.rlim_cur > want.rlim_max) want.rlim_cur = want.rlim_max;
            return real(res, &want);
        }
    }
    return real(res, rl);
}

int setrlimit(__rlimit_resource_t res, const struct rlimit *rl) { return clamp_and_set("setrlimit", res, rl); }
int setrlimit64(__rlimit_resource_t res, const struct rlimit64 *rl) {
    return clamp_and_set("setrlimit64", res, (const struct rlimit *)rl);
}
