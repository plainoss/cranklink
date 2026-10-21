"""Explicit execution models; these do not identify the physical processor."""
from unicorn import Uc, UC_ARCH_ARM, UC_MODE_THUMB, UC_MODE_MCLASS
from unicorn.arm_const import (UC_CPU_ARM_CORTEX_M0, UC_CPU_ARM_CORTEX_M3,
                               UC_CPU_ARM_CORTEX_M4)


CPU_MODELS = {
    'cortex-m0': UC_CPU_ARM_CORTEX_M0,
    'cortex-m3': UC_CPU_ARM_CORTEX_M3,
    'cortex-m4': UC_CPU_ARM_CORTEX_M4,
}
DEFAULT_CPU = 'cortex-m3'


def new_cpu(model=DEFAULT_CPU):
    if model not in CPU_MODELS:
        raise RuntimeError(f'unsupported execution CPU model {model!r}')
    engine = Uc(UC_ARCH_ARM, UC_MODE_THUMB | UC_MODE_MCLASS)
    engine.ctl_set_cpu_model(CPU_MODELS[model])
    return engine
