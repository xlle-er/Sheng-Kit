#ifndef SHENG_ENGINE_H
#define SHENG_ENGINE_H
#include <stdint.h>
#include <stddef.h>
// ABI v1. All calls are thread safe. JSON strings must be freed exactly once.
uint32_t sheng_abi_version(void);
uint64_t sheng_engine_create(void);
void sheng_engine_destroy(uint64_t handle);
char *sheng_engine_start(uint64_t handle, const char *request);
// source 0=system, 1=microphone. Copies PCM16LE mono before returning.
// Returns 0=accepted, -1=invalid/not running, -2=backpressure (session fails).
int32_t sheng_engine_push_pcm(uint64_t handle, uint32_t source, const uint8_t *bytes, size_t length);
void sheng_engine_stop(uint64_t handle);
void sheng_engine_clear(uint64_t handle);
char *sheng_engine_snapshot(uint64_t handle);
char *sheng_engine_drain_confirmed(uint64_t handle);
void sheng_string_free(char *value);
// Resource preparation only occurs through explicit calls, never session start.
uint64_t sheng_resources_start(uint32_t kind, const char *source, const char *target);
char *sheng_resources_poll(uint64_t operation);
void sheng_resources_cancel(uint64_t operation);
char *sheng_language_catalog(void);
char *sheng_validate(const char *request);
int64_t sheng_retry_delay(uint32_t attempt);
uint64_t sheng_probe(const char *request, uint32_t kind);
char *sheng_profile_capabilities(const char *request);
char *sheng_provider_catalog(void);
#endif
