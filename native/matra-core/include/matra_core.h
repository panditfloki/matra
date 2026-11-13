#ifndef MATRA_CORE_H
#define MATRA_CORE_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
/* ABI 1. Borrowed UTF-8 buffers need not be NUL terminated. No ownership crosses this boundary. */
typedef struct { uint64_t input, output, cache_read, cache_write; } MatraCoreTokens;
typedef struct {
    double input, output, cache_read, cache_write;
    uint32_t has_cache_read, has_cache_write;
} MatraCorePriceRates;
typedef struct { double value; uint32_t known; } MatraCoreOptionalAmount;
typedef struct { double amount; uint64_t priced, unknown; } MatraCoreAccumulator;
uint32_t matra_core_abi_version(void);
double matra_core_token_weight(MatraCoreTokens tokens);
double matra_core_weight_output(void);
double matra_core_weight_cache_read(void);
double matra_core_weight_cache_write(void);
uint32_t matra_core_models_match(const uint8_t *left, size_t left_len, const uint8_t *right, size_t right_len);
MatraCoreOptionalAmount matra_core_local_cost(MatraCoreTokens tokens, MatraCorePriceRates rates, uint32_t has_price, double usd_to_inr);
MatraCoreAccumulator matra_core_cost_add(MatraCoreAccumulator accumulator, MatraCoreOptionalAmount cost);
MatraCoreAccumulator matra_core_cost_merge(MatraCoreAccumulator left, MatraCoreAccumulator right);
/* Coverage: 0 none, 1 complete, 2 at least, 3 unavailable. */
uint32_t matra_core_coverage(uint64_t priced, uint64_t unknown);
uint32_t matra_core_claude_headline_rank(const uint8_t *id, size_t len, uint32_t is_balance);
uint32_t matra_core_claude_display_rank(const uint8_t *id, size_t len);
#ifdef __cplusplus
}
#endif
#endif
