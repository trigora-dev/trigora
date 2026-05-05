import { defineFlow } from '@trigora/sdk';
import { StripeWebhookVerificationError, verifyStripeWebhook } from '@trigora/sdk/stripe';

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

export default defineFlow<StripeCheckoutEvent>({
  id: 'stripe-checkout',
  trigger: { type: 'webhook' },

  async run(event, ctx) {
    let stripeEvent: StripeCheckoutEvent;

    try {
      stripeEvent = await verifyStripeWebhook<StripeCheckoutEvent>(event, {
        secret: ctx.env.STRIPE_WEBHOOK_SECRET,
      });
    } catch (error) {
      if (error instanceof StripeWebhookVerificationError) {
        await ctx.log.warn('Rejected Stripe webhook', {
          reason: error.message,
        });

        return new Response('Invalid Stripe signature', {
          status: 400,
        });
      }

      throw error;
    }

    if (stripeEvent.type !== 'checkout.session.completed') {
      return { ok: true, ignored: true };
    }

    const session = stripeEvent.data?.object;

    if (!session?.id) {
      return { ok: false, error: 'Missing checkout session id' };
    }

    await fulfillOrder(session);

    await ctx.log.info('New purchase', {
      email: session.customer_email,
      amount: session.amount_total ? session.amount_total / 100 : null,
      currency: session.currency,
      sessionId: session.id,
    });

    return {
      ok: true,
      received: true,
      sessionId: session.id,
    };
  },
});
