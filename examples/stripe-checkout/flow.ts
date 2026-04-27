import { defineFlow } from '@trigora/sdk';

type StripeCheckoutSession = {
  id: string;
  customer_email?: string | null;
  amount_total?: number | null;
  currency?: string | null;
};

type StripeCheckoutEvent = {
  type?: string;
  data?: {
    object?: StripeCheckoutSession;
  };
};

async function fulfillOrder(session: StripeCheckoutSession): Promise<void> {
  // Replace this with your own business logic.
  // Example: create the order record, provision access, or send a receipt email.
  void session;
}

export default defineFlow({
  id: 'stripe-checkout',
  trigger: { type: 'webhook' },

  async run(event, ctx) {
    const body = event.payload as StripeCheckoutEvent;

    if (body.type !== 'checkout.session.completed') {
      await ctx.log.info('Ignoring Stripe event', {
        eventType: body.type ?? 'unknown',
      });
      return;
    }

    const session = body.data?.object;

    if (!session?.id) {
      await ctx.log.warn('Stripe checkout session is missing an id');
      return;
    }

    await fulfillOrder(session);

    await ctx.log.info('New purchase', {
      email: session.customer_email,
      amount: session.amount_total ? session.amount_total / 100 : null,
      currency: session.currency,
      sessionId: session.id,
    });
  },
});
