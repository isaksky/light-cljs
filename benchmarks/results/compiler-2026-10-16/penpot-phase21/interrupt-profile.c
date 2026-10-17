/* Diagnostic only: interrupt-check samples, not elapsed-time samples. No
 * engine roots, user property access, or live atom pointers are retained. */
#define LIGHT_PROFILE_SIZE 8192
typedef struct {
    char key[768];
    unsigned long long top, inclusive;
} LightProfileEntry;
static LightProfileEntry light_profile[LIGHT_PROFILE_SIZE];
static unsigned long long light_profile_polls, light_profile_dropped;
static int light_profile_enabled = -1;
static void light_profile_dump(void)
{
    fprintf(stderr, "JS_INTERRUPT_PROFILE polls=%llu dropped=%llu\n",
            light_profile_polls, light_profile_dropped);
    for (int i = 0; i < LIGHT_PROFILE_SIZE; i++) {
        const LightProfileEntry *e = &light_profile[i];
        if (e->key[0]) fprintf(stderr, "JS_SAMPLE top=%llu inclusive=%llu %s\n",
                              e->top, e->inclusive, e->key);
    }
}
static void light_profile_sample(JSContext *ctx)
{
    if (light_profile_enabled < 0) {
        light_profile_enabled = getenv("LIGHT_PROFILE_INTERRUPTS") != NULL;
        if (light_profile_enabled) atexit(light_profile_dump);
    }
    if (!light_profile_enabled) return;
    light_profile_polls++;
    int seen[128], count = 0;
    for (JSStackFrame *sf = ctx->rt->current_stack_frame; sf && count < 128; sf = sf->prev_frame) {
        if (JS_VALUE_GET_TAG(sf->cur_func) != JS_TAG_OBJECT) continue;
        JSObject *p = JS_VALUE_GET_OBJ(sf->cur_func);
        if (!js_class_has_bytecode(p->class_id)) continue;
        JSFunctionBytecode *b = p->u.func.function_bytecode;
        char filename[384], name[256], key[768];
        const char *f = JS_AtomGetStrRT(ctx->rt, filename, sizeof(filename), b->filename);
        const char *n = JS_AtomGetStrRT(ctx->rt, name, sizeof(name), b->func_name);
        snprintf(key, sizeof(key), "%s:%d:%d %s", f, b->line_num, b->col_num, n);
        uint32_t hash = 2166136261u;
        for (const unsigned char *s = (const unsigned char *)key; *s; s++) hash = (hash ^ *s) * 16777619u;
        int index = hash % LIGHT_PROFILE_SIZE;
        int probes = 0;
        while (light_profile[index].key[0] && strcmp(light_profile[index].key, key)) {
            index = (index + 1) % LIGHT_PROFILE_SIZE;
            if (++probes == LIGHT_PROFILE_SIZE) break;
        }
        if (probes == LIGHT_PROFILE_SIZE) { light_profile_dropped++; continue; }
        LightProfileEntry *e = &light_profile[index];
        if (!e->key[0]) memcpy(e->key, key, strlen(key) + 1);
        if (count == 0) e->top++;
        bool repeated = false;
        for (int i = 0; i < count; i++) if (seen[i] == index) repeated = true;
        if (!repeated) e->inclusive++;
        seen[count++] = index;
    }
}
