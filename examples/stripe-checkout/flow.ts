import { defineFlow } from '@trigora/sdk';
import { StripeWebhookVerificationError, verifyStripeWebhook } from '@trigora/sdk/stripe';

type CheckoutSession = {
  id: string;
  payment_status?: string | null;
  customer_email?: string | null;
  amount_total?: number | null;
  currency?: string | null;
  metadata?: {
    orderId?: string;
    userId?: string;
  };
};

type StripeEvent = {
  id: string;
  type: string;
  data?: {
    object?: CheckoutSession;
  };
};

async function appRequest(
  path: string,
  init: RequestInit,
  ctx: { env: Record<string, string | undefined> },
) {
  const response = await fetch(`${ctx.env.APP_API_URL}${path}`, {
    ...init,
    headers: {
      Authorization: `Bearer ${ctx.env.APP_API_TOKEN}`,
      'Content-Type': 'application/json',
      ...(init.headers || {}),
    },
  });

  if (!response.ok) {
    const body = await response.text();
    throw new Error(`App API failed (${response.status}): ${body}`);
  }
}

async function markOrderPaid(session: CheckoutSession, eventId: string, ctx: any) {
  await appRequest(
    `/internal/orders/${session.metadata?.orderId}/paid`,
    {
      method: 'POST',
      headers: {
        'Idempotency-Key': eventId,
      },
      body: JSON.stringify({
        stripeSessionId: session.id,
        amountTotal: session.amount_total,
        currency: session.currency,
        userId: session.metadata?.userId,
      }),
    },
    ctx,
  );
}

async function sendSlackNotification(session: CheckoutSession, ctx: any) {
  if (!ctx.env.SLACK_WEBHOOK_URL) return;

  await fetch(ctx.env.SLACK_WEBHOOK_URL, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      text: `Payment received for order ${session.metadata?.orderId ?? 'unknown'}`,
    }),
  });
}

async function sendAnalyticsEvent(session: CheckoutSession, eventId: string, ctx: any) {
  if (!ctx.env.ANALYTICS_ENDPOINT || !ctx.env.ANALYTICS_TOKEN) return;

  await fetch(ctx.env.ANALYTICS_ENDPOINT, {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${ctx.env.ANALYTICS_TOKEN}`,
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({
      event: 'checkout.completed',
      eventId,
      orderId: session.metadata?.orderId,
      userId: session.metadata?.userId,
      sessionId: session.id,
      amountTotal: session.amount_total,
      currency: session.currency,
    }),
  });
}

async function sendReceiptEmail(session: CheckoutSession, ctx: any) {
  if (!ctx.env.EMAIL_API_URL || !ctx.env.EMAIL_API_KEY || !session.customer_email) return;

  await fetch(ctx.env.EMAIL_API_URL, {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${ctx.env.EMAIL_API_KEY}`,
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({
      to: session.customer_email,
      template: 'receipt',
      data: {
        orderId: session.metadata?.orderId,
        amountTotal: session.amount_total,
        currency: session.currency,
      },
    }),
  });
}

export default defineFlow<StripeEvent>({
  id: 'stripe-checkout',
  trigger: { type: 'webhook' },

  async run(event, ctx) {
    let stripeEvent: StripeEvent;

    try {
      stripeEvent = await verifyStripeWebhook<StripeEvent>(event, {
        secret: ctx.env.STRIPE_WEBHOOK_SECRET,
      });
    } catch (error) {
      if (error instanceof StripeWebhookVerificationError) {
        await ctx.log.warn('Rejected webhook signature', {
          reason: error.message,
        });

        return new Response('Invalid signature', { status: 400 });
      }

      throw error;
    }

    if (stripeEvent.type !== 'checkout.session.completed') {
      await ctx.log.info('Ignored Stripe event', {
        type: stripeEvent.type,
        eventId: stripeEvent.id,
      });

      return { ok: true, ignored: true };
    }

    const session = stripeEvent.data?.object;

    if (!session?.id || !session.metadata?.orderId) {
      await ctx.log.warn('Missing required checkout metadata', {
        eventId: stripeEvent.id,
        sessionId: session?.id,
      });

      return new Response('Missing order metadata', { status: 400 });
    }

    if (session.payment_status !== 'paid') {
      await ctx.log.info('Ignoring unpaid checkout session', {
        eventId: stripeEvent.id,
        sessionId: session.id,
        paymentStatus: session.payment_status,
      });

      return { ok: true, ignored: true };
    }

    await markOrderPaid(session, stripeEvent.id, ctx);

    const fanout = await Promise.allSettled([
      sendSlackNotification(session, ctx),
      sendAnalyticsEvent(session, stripeEvent.id, ctx),
      sendReceiptEmail(session, ctx),
    ]);

    const failures = fanout
      .map((result, index) => ({ result, index }))
      .filter(
        (entry): entry is { result: PromiseRejectedResult; index: number } =>
          entry.result.status === 'rejected',
      );

    if (failures.length > 0) {
      await ctx.log.warn('Non-critical downstream task failed', {
        eventId: stripeEvent.id,
        failedTasks: failures.map((failure) => failure.index),
      });
    }

    await ctx.log.info('Checkout processed', {
      eventId: stripeEvent.id,
      sessionId: session.id,
      orderId: session.metadata.orderId,
      email: session.customer_email,
      amountTotal: session.amount_total,
      currency: session.currency,
    });

    return {
      ok: true,
      orderId: session.metadata.orderId,
      sessionId: session.id,
    };
  },
});
