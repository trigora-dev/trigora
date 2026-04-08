import { defineFlow } from '@trigora/sdk';

export default defineFlow({
  id: 'payment',
  trigger: { type: 'manual' },
  async run(event, ctx) {
    await ctx.log.info('processing payment', event.payload);
  },
});
