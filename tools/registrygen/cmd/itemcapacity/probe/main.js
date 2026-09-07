import { ItemStack, ItemTypes, system } from '@minecraft/server';

system.run(() => {
  const types = ItemTypes.getAll().sort((a, b) => a.id.localeCompare(b.id));
  let offset = 0;
  let failures = 0;
  const run = system.runInterval(() => {
    const end = Math.min(offset + 32, types.length);
    for (; offset < end; offset++) {
      const id = types[offset].id;
      try {
        const stack = new ItemStack(id, 1);
        console.warn('CINNABAR_CAPACITY_ROW ' + JSON.stringify([id, stack.maxAmount, stack.isStackable]));
      } catch (error) {
        failures++;
        console.warn('CINNABAR_CAPACITY_ERROR ' + JSON.stringify([id, String(error)]));
      }
    }
    if (offset === types.length) {
      console.warn('CINNABAR_CAPACITY_DONE ' + JSON.stringify({ total: types.length, failures }));
      system.clearRun(run);
    }
  }, 1);
});
