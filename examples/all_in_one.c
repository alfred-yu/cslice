/*
 * cslice 全场景演示：本函数覆盖切片引擎支持的全部场景。
 * 运行 examples/showcase.py 查看切分效果。
 */
#include <stdint.h>
#include <stddef.h>

#define FRAMES_MAX 64

static int g_calls = 0;

static void log_error(const char *msg);
static void blend_frames_ref(const uint8_t *src, uint8_t *dst, int width);
static void blend_frames_sse(const uint8_t *src, uint8_t *dst, int width);

static int blend_frames(const uint8_t *src, int width, int height, int mode, uint8_t *dst)
{
    int result = -1;                                // 声明带初始化：独立成片
    int x;
    int y;                                          // 纯声明：不归属任何片
    uint32_t sum = 0u;

#ifdef ENABLE_SSE                                   // 条件编译块：整体一片
    blend_frames_sse(src, dst, width);
#else
    blend_frames_ref(src, dst, width);
#endif

    if (src == NULL || width <= 0 || height <= 0)   // 分支片：体内嵌套语句随分支整体
    {
        log_error("blend_frames: bad args");
        return -2;                                  // 分支内 return 不独立成片
    }

    for (int row = 0;                               // 多行 for 头：loop 片跨三行
         row < height;
         row++)
    {
        sum += (uint32_t)src[row * width];          // 循环体直接语句：计算片（携带父循环条件）
        switch (mode)                               // switch：每 case 一片，头并入首 case
        {
        case 0:
            dst[row] = (uint8_t)(sum & 0xFFu);
            break;                                  // break 归 case 片
        case 1:
            dst[row] = (uint8_t)(sum >> 8);
            if (dst[row] > 200u)                    // case 内嵌套 if：随 case 片（不递归拆）
            {
                dst[row] = 200u;
            }
            break;
        default:
            continue;                               // default 片
        }
        x = row;                                    // 嵌套块之后的直接语句：计算片
    }

    while (sum > 0xFFu)                             // 循环头独立成片
    {
        sum >>= 1;                                  // 复合赋值：计算片
        if (sum == 0x7Fu)                           // 循环内嵌套 if：独立 branch 片
        {
            result = 1;
        }
    }

    do                                              // do-while：do 行为头片
    {
        sum = sum * 3u + 1u;
    } while (sum != 1u);                            // 收尾 while 条件行不归属任何片

    if (result == 1)                                // if/else-if/else 链：逐分支拆片
    {
        result = 100;
    }
    else if (result == 0)
    {
        result = 200;
    }
    else
    {
        result = 300;
    }

    for (int i = 0; i < 4; i++)                     // 三层嵌套循环：每层头各一片
    {
        for (int j = 0; j < 4; j++)
        {
            for (int k = 0; k < 4; k++)
            {
                dst[i * 16u + j * 4u + k] = (uint8_t)(k + 1);   // 最内层直接语句成片
            }
        }
    }

    g_calls++;                                      // 连续语句聚合为计算片
    (void)x;
    (void)y;
    return result;                                  // 顶层 return 独立成片
}
