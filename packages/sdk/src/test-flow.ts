import { defineFlow } from './index';

const flow = defineFlow({
  id: 'my flow',
  trigger: { type: 'manual' },
  run: async (event, ctx) => {
    await ctx.log.info('Hello from flow', { payload: event.payload });
  },
});

export default flow;
